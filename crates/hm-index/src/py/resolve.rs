//! Turns the facts of a Python file into edges and refs: imports by package roots,
//! calls by local name, import binding, module path, class, `self` and stated attribute
//! type, and `extends` by base class. A reference that does not name exactly one
//! target becomes a ref, never an edge; a call to a builtin becomes neither.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use hm_core::ids::file_id;

use super::modules::{PyModule, PyModules, PyTarget};
use crate::facts::{CallFact, FileFacts, ImportFact, ImportedName, SymbolKind};
use crate::ts::resolve::{EdgeRow, RefRow, ResolvedFile, symbol_ids};

/// How many bindings, re-exports and base classes a name chain follows (spec §6.2).
const MAX_HOPS: u32 = 5;

/// The builtins and builtin exceptions a call or base may name without a ref.
const BUILTINS: &[&str] = &[
    "abs",
    "all",
    "any",
    "ascii",
    "bin",
    "bool",
    "breakpoint",
    "bytearray",
    "bytes",
    "callable",
    "chr",
    "classmethod",
    "compile",
    "complex",
    "delattr",
    "dict",
    "dir",
    "divmod",
    "enumerate",
    "eval",
    "exec",
    "filter",
    "float",
    "format",
    "frozenset",
    "getattr",
    "globals",
    "hasattr",
    "hash",
    "help",
    "hex",
    "id",
    "input",
    "int",
    "isinstance",
    "issubclass",
    "iter",
    "len",
    "list",
    "locals",
    "map",
    "max",
    "memoryview",
    "min",
    "next",
    "object",
    "oct",
    "open",
    "ord",
    "pow",
    "print",
    "property",
    "range",
    "repr",
    "reversed",
    "round",
    "set",
    "setattr",
    "slice",
    "sorted",
    "staticmethod",
    "str",
    "sum",
    "super",
    "tuple",
    "type",
    "vars",
    "zip",
    "__import__",
    "BaseException",
    "Exception",
    "ArithmeticError",
    "AssertionError",
    "AttributeError",
    "ConnectionError",
    "FileExistsError",
    "FileNotFoundError",
    "ImportError",
    "IndexError",
    "KeyError",
    "LookupError",
    "ModuleNotFoundError",
    "NotImplementedError",
    "OSError",
    "PermissionError",
    "RuntimeError",
    "StopIteration",
    "TimeoutError",
    "TypeError",
    "UnicodeDecodeError",
    "UnicodeEncodeError",
    "ValueError",
    "ZeroDivisionError",
];

/// The Python files of a worktree with their facts, and the module lookup.
pub struct PyProject<'a> {
    /// Relative path to the facts of that file.
    pub facts: &'a BTreeMap<String, FileFacts>,
    pub modules: &'a PyModules,
}

/// A `name_in` lookup: `(module, name, submodules, hops)`.
type NameKey = (ModuleKey, String, bool, u32);

/// The lookups of one file's resolution. They note every file whose facts they read,
/// so the file's `reverse_import` rows name every file whose change can reach it,
/// and remember each name lookup in a module, so a star-import graph is searched
/// once per name, not once per path through it.
struct Look<'a> {
    facts: &'a BTreeMap<String, FileFacts>,
    modules: &'a PyModules,
    read: RefCell<BTreeSet<String>>,
    names: RefCell<HashMap<NameKey, Option<Named>>>,
    /// The module files whose star imports are being searched, outermost first.
    star_path: RefCell<Vec<String>>,
}

/// What a name stands for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Named {
    /// A top-level symbol: the file that declares it and the index into its `symbols`.
    Symbol(String, usize),
    Module(ModuleKey),
    /// Something from a package outside the worktree, by its first segment.
    External(String),
}

/// A `PyModule` that can be ordered, to tell star-import hits apart.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ModuleKey {
    file: Option<String>,
    package_dir: Option<String>,
}

impl From<PyModule> for ModuleKey {
    fn from(m: PyModule) -> Self {
        ModuleKey {
            file: m.file,
            package_dir: m.package_dir,
        }
    }
}

fn target_named(target: PyTarget) -> Option<Named> {
    match target {
        PyTarget::Module(m) => Some(Named::Module(m.into())),
        PyTarget::External(first) => Some(Named::External(first)),
        PyTarget::Unresolved => None,
    }
}

/// `import a.b` (one name, imported as `*`, bound to a local name), not `from m import *`.
fn is_plain_import(i: &ImportFact) -> bool {
    matches!(&i.names[..], [n] if n.imported == "*" && n.local != "*")
}

fn first_segment(dotted: &str) -> &str {
    dotted.split('.').next().unwrap_or(dotted)
}

fn is_ident(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// The leading identifier of an expression: `Service` for `Service(Mailer())`.
fn root_ident(expr: &str) -> &str {
    let end = expr
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(expr.len());
    &expr[..end]
}

/// The last top-level symbol called `name`: of two definitions the later one wins.
fn last_top_level(facts: &FileFacts, name: &str) -> Option<usize> {
    facts
        .symbols
        .iter()
        .rposition(|s| s.owner.is_empty() && s.name == name)
}

/// The last import of `facts` that binds the local name `name`.
fn import_binding<'f>(
    facts: &'f FileFacts,
    name: &str,
) -> Option<(&'f ImportFact, &'f ImportedName)> {
    facts.imports.iter().rev().find_map(|i| {
        let n = i.names.iter().find(|n| n.local == name && n.local != "*")?;
        Some((i, n))
    })
}

/// Whether a symbol or an import of `facts` binds `name`, so it is not the builtin.
fn binds(facts: &FileFacts, name: &str) -> bool {
    last_top_level(facts, name).is_some() || import_binding(facts, name).is_some()
}

fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// A symbol a call can name.
fn callable(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Class | SymbolKind::Variable
    )
}

impl<'a> Look<'a> {
    fn new(p: &PyProject<'a>) -> Look<'a> {
        Look {
            facts: p.facts,
            modules: p.modules,
            read: RefCell::new(BTreeSet::new()),
            names: RefCell::new(HashMap::new()),
            star_path: RefCell::new(Vec::new()),
        }
    }

    /// The facts of `rel`, noted as read.
    fn get(&self, rel: &str) -> Option<&'a FileFacts> {
        let facts = self.facts.get(rel)?;
        if !self.read.borrow().contains(rel) {
            self.read.borrow_mut().insert(rel.to_string());
        }
        Some(facts)
    }

    /// The facts of `rel`, which a lookup already found, noted as read.
    fn at(&self, rel: &str) -> &'a FileFacts {
        self.get(rel).expect("a file a lookup found has facts")
    }

    fn kind(&self, file: &str, symbol: usize) -> SymbolKind {
        self.at(file).symbols[symbol].kind
    }

    /// The last import of `rel` that binds `name`. `from . import name` in a package's
    /// own `__init__` names the same module and name again: it binds no name of its
    /// own, so the lookup goes on to the submodule.
    fn own_binding<'f>(
        &self,
        rel: &str,
        facts: &'f FileFacts,
        name: &str,
    ) -> Option<(&'f ImportFact, &'f ImportedName)> {
        facts.imports.iter().rev().find_map(|i| {
            let n = i.names.iter().find(|n| n.local == name && n.local != "*")?;
            let loops = n.imported == name
                && !is_plain_import(i)
                && matches!(
                    self.modules.resolve(rel, &i.specifier),
                    PyTarget::Module(m) if m.file.as_deref() == Some(rel)
                );
            (!loops).then_some((i, n))
        })
    }

    /// Whether `name` is a name of the module file `rel` itself: a top-level symbol or
    /// an import that leads elsewhere.
    fn bound_in(&self, rel: &str, facts: &FileFacts, name: &str) -> bool {
        last_top_level(facts, name).is_some() || self.own_binding(rel, facts, name).is_some()
    }

    /// What the import `i` of `rel` binds to its local name `n`. `import a.b` binds
    /// `a` to the package `a`; `import a.b as c` binds `c` to `a.b`.
    fn follow_binding(
        &self,
        rel: &str,
        i: &ImportFact,
        n: &ImportedName,
        hops: u32,
    ) -> Option<Named> {
        if n.imported == "*" {
            let first = first_segment(&i.specifier);
            let spec = if n.local == first {
                first
            } else {
                &i.specifier
            };
            return target_named(self.modules.resolve(rel, spec));
        }
        match target_named(self.modules.resolve(rel, &i.specifier))? {
            Named::Module(m) => self.name_in(&m, &n.imported, hops + 1, true),
            other @ Named::External(_) => Some(other),
            Named::Symbol(..) => None,
        }
    }

    /// The name `name` in module `m`: its last top-level symbol of that name, else an
    /// import that binds it (followed), else (with `submodules`, for a package) its
    /// submodule, else the unique hit among its star imports.
    fn name_in(&self, m: &ModuleKey, name: &str, hops: u32, submodules: bool) -> Option<Named> {
        if hops > MAX_HOPS {
            return None;
        }
        let key = (m.clone(), name.to_string(), submodules, hops);
        if let Some(known) = self.names.borrow().get(&key) {
            return known.clone();
        }
        let found = self.name_in_module(m, name, hops, submodules);
        self.names.borrow_mut().insert(key, found.clone());
        found
    }

    /// [`name_in`](Self::name_in) without the memo.
    fn name_in_module(
        &self,
        m: &ModuleKey,
        name: &str,
        hops: u32,
        submodules: bool,
    ) -> Option<Named> {
        let file = m.file.as_deref().and_then(|f| Some((f, self.get(f)?)));
        if let Some((rel, facts)) = file {
            if let Some(i) = last_top_level(facts, name) {
                return Some(Named::Symbol(rel.to_string(), i));
            }
            if let Some((i, n)) = self.own_binding(rel, facts, name) {
                return self.follow_binding(rel, i, n, hops);
            }
        }
        if submodules
            && let Some(dir) = &m.package_dir
            && let Some(sub) = self.modules.submodule(dir, name)
        {
            return Some(Named::Module(sub.into()));
        }
        let (rel, facts) = file?;
        self.star_hit(rel, facts, name, hops)
    }

    /// The one thing the star imports of `rel` offer for `name`; `None` when none or
    /// two different ones do. A name that starts with `_` is never carried. A module
    /// whose star imports are already being searched further out is skipped: going
    /// round a star-import cycle offers nothing that search does not.
    fn star_hit(&self, rel: &str, facts: &FileFacts, name: &str, hops: u32) -> Option<Named> {
        if name.starts_with('_') {
            return None;
        }
        self.star_path.borrow_mut().push(rel.to_string());
        let mut hits = BTreeSet::new();
        for i in &facts.imports {
            if !i.names.iter().any(|n| n.local == "*") {
                continue;
            }
            let Some(Named::Module(m)) = target_named(self.modules.resolve(rel, &i.specifier))
            else {
                continue;
            };
            let on_path = m
                .file
                .as_deref()
                .is_some_and(|f| self.star_path.borrow().iter().any(|p| p == f));
            if !on_path && let Some(hit) = self.name_in(&m, name, hops + 1, false) {
                hits.insert(hit);
            }
        }
        self.star_path.borrow_mut().pop();
        let mut hits = hits.into_iter();
        match (hits.next(), hits.next()) {
            (Some(hit), None) => Some(hit),
            _ => None,
        }
    }

    /// The name `name` as seen in file `rel`, and whether it is the file's own symbol.
    fn local_name(&self, rel: &str, facts: &FileFacts, name: &str) -> Option<(Named, bool)> {
        if let Some(i) = last_top_level(facts, name) {
            return Some((Named::Symbol(rel.to_string(), i), true));
        }
        if let Some((i, n)) = self.own_binding(rel, facts, name) {
            return Some((self.follow_binding(rel, i, n, 0)?, false));
        }
        Some((self.star_hit(rel, facts, name, 0)?, false))
    }

    /// Walks `rest` (submodules or names) from `start`. Anything below an external
    /// package stays external.
    fn walk(&self, start: Named, rest: &[&str]) -> Option<Named> {
        let mut at = start;
        for seg in rest {
            at = match at {
                Named::Module(m) => self.name_in(&m, seg, 0, true)?,
                Named::External(_) => return Some(at),
                Named::Symbol(..) => return None,
            };
        }
        Some(at)
    }

    /// The dotted path `a.b.C` as seen in file `rel`.
    fn path_in(&self, rel: &str, path: &str) -> Option<Named> {
        let facts = self.get(rel)?;
        let segs: Vec<&str> = path.split('.').collect();
        if !segs.iter().all(|s| is_ident(s)) {
            return None;
        }
        let (first, _) = self.local_name(rel, facts, segs[0])?;
        self.walk(first, &segs[1..])
    }

    /// The class `path` names in `rel`, if it names one.
    fn class_in(&self, rel: &str, path: &str) -> Option<(String, usize)> {
        match self.path_in(rel, path)? {
            Named::Symbol(file, i) if self.kind(&file, i) == SymbolKind::Class => Some((file, i)),
            _ => None,
        }
    }

    /// The last method `name` of class `class` (a top-level symbol of `file`).
    fn method_of(&self, file: &str, class: usize, name: &str) -> Option<usize> {
        let facts = self.at(file);
        let owner = &facts.symbols[class].name;
        facts
            .symbols
            .iter()
            .rposition(|s| s.kind == SymbolKind::Method && s.owner == *owner && s.name == name)
    }

    /// The bases of a class that resolve to classes, left to right.
    fn bases(&self, file: &str, class: usize) -> Vec<(String, usize)> {
        let facts = self.at(file);
        let name = &facts.symbols[class].name;
        facts
            .heritage
            .iter()
            .filter(|h| h.class == *name)
            .filter_map(|h| self.class_in(file, &h.extends))
            .collect()
    }

    /// The first method `name` on the bases of a class, depth first, left to right.
    fn inherited(
        &self,
        file: &str,
        class: usize,
        name: &str,
        depth: u32,
    ) -> Option<(String, usize)> {
        if depth >= MAX_HOPS {
            return None;
        }
        for (base_file, base) in self.bases(file, class) {
            if let Some(m) = self.method_of(&base_file, base, name) {
                return Some((base_file, m));
            }
            if let Some(found) = self.inherited(&base_file, base, name, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    /// Method `name` of a class or, failing that, of its bases.
    fn method_or_inherited(&self, file: &str, class: usize, name: &str) -> Option<(String, usize)> {
        match self.method_of(file, class, name) {
            Some(m) => Some((file.to_string(), m)),
            None => self.inherited(file, class, name, 0),
        }
    }

    /// The one class the attribute facts for `attr` on a class state, or, when the
    /// class states none, on its bases (depth first). Every fact of the class that
    /// decides must name that same class: one that names no class, or another one,
    /// leaves the type unsure.
    fn attr_type(&self, file: &str, class: usize, attr: &str, depth: u32) -> AttrType {
        let facts = self.at(file);
        let name = &facts.symbols[class].name;
        let stated: Vec<&str> = facts
            .attrs
            .iter()
            .filter(|a| a.class == *name && a.attr == attr)
            .map(|a| a.type_name.as_str())
            .collect();
        if !stated.is_empty() {
            let mut types = BTreeSet::new();
            for ty in stated {
                match self.class_in(file, ty) {
                    Some(k) => types.insert(k),
                    None => return AttrType::Unsure,
                };
            }
            let mut types = types.into_iter();
            return match (types.next(), types.next()) {
                (Some((file, k)), None) => AttrType::One(file, k),
                _ => AttrType::Unsure,
            };
        }
        if depth >= MAX_HOPS {
            return AttrType::Unstated;
        }
        for (base_file, base) in self.bases(file, class) {
            match self.attr_type(&base_file, base, attr, depth + 1) {
                AttrType::Unstated => {}
                decided => return decided,
            }
        }
        AttrType::Unstated
    }
}

/// What the attribute facts say about the type of an attribute.
enum AttrType {
    /// No class on the way states a type.
    Unstated,
    /// One class: its file and index.
    One(String, usize),
    /// A stated type that names no class, or two different classes.
    Unsure,
}

/// What a call or base resolved to.
enum Outcome {
    Edge {
        file: String,
        symbol: usize,
        rule: &'static str,
        inferred: bool,
    },
    Ref {
        status: &'static str,
    },
    /// A builtin: neither an edge nor a ref.
    Nothing,
}

const UNRESOLVED: Outcome = Outcome::Ref {
    status: "unresolved",
};
const EXTERNAL: Outcome = Outcome::Ref { status: "external" };

fn sure(file: String, symbol: usize, rule: &'static str) -> Outcome {
    Outcome::Edge {
        file,
        symbol,
        rule,
        inferred: false,
    }
}

fn probably(file: String, symbol: usize, rule: &'static str) -> Outcome {
    Outcome::Edge {
        file,
        symbol,
        rule,
        inferred: true,
    }
}

/// Resolves one file's facts to edges and refs.
pub fn resolve_file(p: &PyProject, rel: &str) -> ResolvedFile {
    let Some(facts) = p.facts.get(rel) else {
        return ResolvedFile::default();
    };
    let look = Look::new(p);
    let mut r = Resolver {
        p: &look,
        rel,
        facts,
        file: file_id(rel),
        ids: HashMap::new(),
        edges: BTreeMap::new(),
        refs: BTreeSet::new(),
    };
    let mut imports_files = BTreeSet::new();

    for i in &facts.imports {
        for target in r.import_targets(i) {
            match target {
                Ok(file) => {
                    r.edge(
                        "imports",
                        r.file.clone(),
                        file_id(&file),
                        i.line,
                        "py.import",
                        false,
                    );
                    imports_files.insert(file);
                }
                Err((text, status)) => r.reference("import", text, i.line, status),
            }
        }
    }

    for c in &facts.calls {
        let src = match &c.owner {
            Some((owner, name)) => match facts.holder(owner, name, c.line) {
                Some(i) => r.symbol_id_of(rel, i),
                None => r.file.clone(),
            },
            None => r.file.clone(),
        };
        match r.resolve_call(c) {
            Outcome::Edge {
                file,
                symbol,
                rule,
                inferred,
            } => {
                let dst = r.symbol_id_of(&file, symbol);
                r.edge("calls", src, dst, c.line, rule, inferred);
            }
            Outcome::Ref { status } => r.reference("call", call_text(c), c.line, status),
            Outcome::Nothing => {}
        }
    }

    for h in &facts.heritage {
        let class = facts.symbols.iter().position(|s| {
            s.kind == SymbolKind::Class
                && s.owner.is_empty()
                && s.name == h.class
                && s.l0 <= h.line
                && h.line <= s.l1
        });
        let Some(class) = class else {
            r.reference("extends", h.extends.clone(), h.line, "unresolved");
            continue;
        };
        match r.resolve_base(&h.extends) {
            Outcome::Edge { file, symbol, .. } => {
                let src = r.symbol_id_of(rel, class);
                let dst = r.symbol_id_of(&file, symbol);
                r.edge("extends", src, dst, h.line, "py.extends", false);
            }
            Outcome::Ref { status } => r.reference("extends", h.extends.clone(), h.line, status),
            Outcome::Nothing => {}
        }
    }

    ResolvedFile {
        edges: r
            .edges
            .into_iter()
            .map(|((kind, src_id, dst_id, line), (rule, inferred))| EdgeRow {
                kind,
                src_id,
                dst_id,
                line,
                rule,
                inferred,
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
        imports_files: {
            // Every file whose facts the resolution read: a change to one can change
            // the result. The files the imports point to are among them.
            let mut read = look.read.take();
            read.extend(imports_files);
            read.remove(rel);
            read
        },
    }
}

/// `receiver.callee`, or `callee` without a receiver.
fn call_text(c: &CallFact) -> String {
    match &c.receiver {
        Some(r) => format!("{r}.{}", c.callee),
        None => c.callee.clone(),
    }
}

type EdgeKey = (&'static str, String, String, u32);

struct Resolver<'a, 'p> {
    p: &'a Look<'p>,
    rel: &'a str,
    facts: &'a FileFacts,
    file: String,
    /// Symbol ids by file, filled when a file is first needed.
    ids: HashMap<String, Vec<String>>,
    edges: BTreeMap<EdgeKey, (&'static str, bool)>,
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
        inferred: bool,
    ) {
        self.edges
            .entry((kind, src, dst, line))
            .or_insert((rule, inferred));
    }

    fn reference(&mut self, kind: &'static str, text: String, line: u32, status: &'static str) {
        self.refs.insert((kind, text, line, status));
    }

    fn symbol_id_of(&mut self, rel: &str, index: usize) -> String {
        if !self.ids.contains_key(rel) {
            let ids = self
                .p
                .get(rel)
                .map(|f| symbol_ids(rel, f))
                .unwrap_or_default();
            self.ids.insert(rel.to_string(), ids);
        }
        self.ids[rel][index].clone()
    }

    /// The files an import points to (`Ok`), or the ref it makes (`Err`).
    /// `from m import n` points to `m`'s file when `n` is a name bound in it and to
    /// the submodule's file when `n` is a submodule; a namespace package has no file.
    fn import_targets(&self, i: &ImportFact) -> Vec<Result<String, (String, &'static str)>> {
        let (p, rel) = (self.p, self.rel);
        let m = match p.modules.resolve(rel, &i.specifier) {
            PyTarget::Module(m) => m,
            PyTarget::External(first) => return vec![Err((first, "external"))],
            PyTarget::Unresolved => return vec![Err((i.specifier.clone(), "unresolved"))],
        };
        if is_plain_import(i) {
            return m.file.into_iter().map(Ok).collect();
        }
        let own = m.file.as_deref().and_then(|f| Some((f, p.get(f)?)));
        let mut out = Vec::new();
        for n in &i.names {
            let bound = n.imported == "*"
                || own.is_some_and(|(file, facts)| p.bound_in(file, facts, &n.imported));
            let sub = m
                .package_dir
                .as_deref()
                .filter(|_| !bound)
                .and_then(|dir| p.modules.submodule(dir, &n.imported));
            let file = match sub {
                Some(sub) => sub.file,
                None => m.file.clone(),
            };
            out.extend(file.map(Ok));
        }
        out
    }

    /// The symbol a call names, as a call outcome: a class gives `py.call.class`.
    fn call_to(&self, file: String, symbol: usize, rule: &'static str) -> Outcome {
        match self.p.kind(&file, symbol) {
            SymbolKind::Class => sure(file, symbol, "py.call.class"),
            kind if callable(kind) => sure(file, symbol, rule),
            _ => UNRESOLVED,
        }
    }

    /// A name that nothing binds: a builtin makes nothing, anything else a ref.
    fn unbound(&self, name: &str) -> Outcome {
        if is_builtin(name) && !binds(self.facts, name) {
            Outcome::Nothing
        } else {
            UNRESOLVED
        }
    }

    fn resolve_call(&self, c: &CallFact) -> Outcome {
        let (p, rel, facts) = (self.p, self.rel, self.facts);
        let callee = c.callee.as_str();
        // The class whose method holds the call, for `self`, `cls` and `super()`.
        let class =
            c.owner
                .as_ref()
                .map(|(owner, _)| owner.as_str())
                .filter(|o| !o.is_empty())
                .and_then(|o| {
                    facts.symbols.iter().rposition(|s| {
                        s.kind == SymbolKind::Class && s.owner.is_empty() && s.name == o
                    })
                });

        let Some(receiver) = c.receiver.as_deref() else {
            // `f()`, `K()`.
            return match p.local_name(rel, facts, callee) {
                Some((Named::Symbol(file, i), local)) => {
                    let rule = if local {
                        "py.call.local"
                    } else {
                        "py.call.import"
                    };
                    self.call_to(file, i, rule)
                }
                Some((Named::External(_), _)) => EXTERNAL,
                Some((Named::Module(_), _)) => UNRESOLVED,
                None => self.unbound(callee),
            };
        };

        // `super().m()`.
        if receiver == "super()" || receiver.starts_with("super(") {
            return match class.and_then(|k| p.inherited(rel, k, callee, 0)) {
                Some((file, m)) => probably(file, m, "py.call.inherited"),
                None => UNRESOLVED,
            };
        }

        let segs: Vec<&str> = receiver.split('.').collect();
        if !segs.iter().all(|s| is_ident(s)) {
            // `Service(Mailer()).place()`: only the root says where it came from.
            let root = root_ident(receiver);
            return match (!root.is_empty())
                .then(|| p.local_name(rel, facts, root))
                .flatten()
            {
                Some((Named::External(_), _)) => EXTERNAL,
                _ => UNRESOLVED,
            };
        }

        // `self.m()`, `cls.m()`, `self.x.y()` in a method.
        if let Some(k) = class
            && matches!(segs[0], "self" | "cls")
        {
            return match segs[..] {
                [_] => match p.method_of(rel, k, callee) {
                    Some(m) => sure(rel.to_string(), m, "py.call.self"),
                    None => match p.inherited(rel, k, callee, 0) {
                        Some((file, m)) => probably(file, m, "py.call.inherited"),
                        None => UNRESOLVED,
                    },
                },
                [_, attr] => match p.attr_type(rel, k, attr, 0) {
                    AttrType::One(file, ty) => match p.method_or_inherited(&file, ty, callee) {
                        Some((file, m)) => probably(file, m, "py.call.attr"),
                        None => UNRESOLVED,
                    },
                    _ => UNRESOLVED,
                },
                _ => UNRESOLVED,
            };
        }

        // `m.f()`, `a.b.f()`, `K.m()`.
        let Some((first, _)) = p.local_name(rel, facts, segs[0]) else {
            return match segs[..] {
                [only] => self.unbound(only),
                _ => UNRESOLVED,
            };
        };
        match p.walk(first, &segs[1..]) {
            Some(Named::Module(m)) => match p.name_in(&m, callee, 0, true) {
                Some(Named::Symbol(file, i)) => self.call_to(file, i, "py.call.module"),
                Some(Named::External(_)) => EXTERNAL,
                _ => UNRESOLVED,
            },
            Some(Named::Symbol(file, k)) if p.kind(&file, k) == SymbolKind::Class => {
                match p.method_of(&file, k, callee) {
                    Some(m) => sure(file, m, "py.call.static"),
                    None => UNRESOLVED,
                }
            }
            Some(Named::External(_)) => EXTERNAL,
            _ => UNRESOLVED,
        }
    }

    /// A base in `class C(B)`: `B` resolved like a call target, when it is a class.
    fn resolve_base(&self, base: &str) -> Outcome {
        match self.p.path_in(self.rel, base) {
            Some(Named::Symbol(file, i)) if self.p.kind(&file, i) == SymbolKind::Class => {
                sure(file, i, "py.extends")
            }
            Some(Named::External(_)) => EXTERNAL,
            Some(_) => UNRESOLVED,
            None if !base.contains('.') => self.unbound(base),
            None => UNRESOLVED,
        }
    }
}
