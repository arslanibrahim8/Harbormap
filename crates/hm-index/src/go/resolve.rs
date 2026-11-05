//! Turns the facts of a Go file into edges and refs. Imports resolve through the
//! `go.mod` files to package folders; calls resolve within the caller's package,
//! through imported packages, through the receiver of the enclosing method, and
//! last by method name across the repository (`Probably`, `inferred`): only a name
//! that an interface of the project declares, and only when one concrete method has
//! it. A reference that does not name exactly one target is a ref, never an edge.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use hm_core::ids::file_id;

use super::gomod::{GoModules, GoTarget, is_test, outside_builds, split};
use crate::facts::{CallFact, FileFacts, SymbolFact, SymbolKind};
use crate::ts::resolve::{EdgeRow, RefRow, ResolvedFile, symbol_ids};

/// Calls to these make neither an edge nor a ref.
/// Every rule a Go edge is written under. These rows belong to the resolver (see
/// [`crate::langs::resolver_rules`]).
pub const RULES: &[&str] = &[
    "go.import",
    "go.call.local",
    "go.call.package",
    "go.call.receiver",
    "go.call.method",
];

const BUILTINS: &[&str] = &[
    "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make", "max",
    "min", "new", "panic", "print", "println", "real", "recover",
];

/// Conversions to these make neither an edge nor a ref.
const PREDECLARED_TYPES: &[&str] = &[
    "any",
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
];

/// The Method symbols of every `.go` file by name, so that one build serves a pass.
#[derive(Debug, Clone, Default)]
pub struct MethodIndex {
    /// Method name to the concrete methods of that name as `(file, symbol index)`,
    /// in file order and then source order. An interface's method specs are not in it.
    by_name: HashMap<String, Vec<(String, usize)>>,
    /// The method names that the interfaces of non-test files declare.
    declared: HashSet<String>,
}

impl MethodIndex {
    /// Lists the Method symbols of the `.go` files of `facts` that a build can hold:
    /// not those under `testdata` or a folder starting with `_` or `.`.
    pub fn build(facts: &BTreeMap<String, FileFacts>) -> MethodIndex {
        let mut by_name: HashMap<String, Vec<(String, usize)>> = HashMap::new();
        let mut declared = HashSet::new();
        for (rel, f) in facts
            .iter()
            .filter(|(rel, _)| rel.ends_with(".go") && !outside_builds(rel))
        {
            let interfaces: HashSet<&str> = f
                .symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Interface && s.owner.is_empty())
                .map(|s| s.name.as_str())
                .collect();
            for (i, s) in f.symbols.iter().enumerate() {
                if s.kind != SymbolKind::Method {
                    continue;
                }
                if interfaces.contains(s.owner.as_str()) {
                    if !is_test(rel) {
                        declared.insert(s.name.clone());
                    }
                } else {
                    by_name
                        .entry(s.name.clone())
                        .or_default()
                        .push((rel.clone(), i));
                }
            }
        }
        MethodIndex { by_name, declared }
    }

    fn named(&self, name: &str) -> &[(String, usize)] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }

    /// Whether an interface of a non-test file declares a method `name`.
    fn declared(&self, name: &str) -> bool {
        self.declared.contains(name)
    }
}

/// The files of a worktree with their facts, its modules and its methods.
pub struct GoProject<'a> {
    pub facts: &'a BTreeMap<String, FileFacts>,
    pub modules: &'a GoModules,
    pub methods: &'a MethodIndex,
}

/// The other `.go` files with facts in `rel`'s folder.
pub fn package_siblings(facts: &BTreeMap<String, FileFacts>, rel: &str) -> Vec<String> {
    let (dir, _) = split(rel);
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    facts
        .range(prefix.clone()..)
        .map(|(k, _)| k)
        .take_while(|k| k.starts_with(&prefix))
        .filter(|k| k.as_str() != rel && k.ends_with(".go") && !k[prefix.len()..].contains('/'))
        .cloned()
        .collect()
}

/// The names of the Method symbols of `facts`.
pub fn method_names(facts: &FileFacts) -> BTreeSet<String> {
    facts
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Method)
        .map(|s| s.name.clone())
        .collect()
}

/// Whether `facts` has a call with a receiver whose callee is in `names`.
pub fn calls_any_method(facts: &FileFacts, names: &BTreeSet<String>) -> bool {
    facts
        .calls
        .iter()
        .any(|c| c.receiver.is_some() && names.contains(&c.callee))
}

/// What an import's local name stands for.
enum Bound<'a> {
    /// The files of an imported package of the worktree.
    Package(Vec<&'a str>),
    External,
    Unresolved,
}

/// What a call resolved to.
enum Outcome {
    Edge {
        file: String,
        symbol: usize,
        rule: &'static str,
    },
    Ref {
        status: &'static str,
    },
    /// A builtin or a conversion.
    Nothing,
}

const UNRESOLVED: Outcome = Outcome::Ref {
    status: "unresolved",
};
const EXTERNAL: Outcome = Outcome::Ref { status: "external" };

/// Resolves one Go file's facts to edges and refs.
pub fn resolve_file(p: &GoProject, rel: &str) -> ResolvedFile {
    let Some(facts) = p.facts.get(rel) else {
        return ResolvedFile::default();
    };
    let (dir, _) = split(rel);
    let caller_is_test = is_test(rel);
    let package: Vec<&str> = p
        .modules
        .files_in(dir)
        .into_iter()
        .filter(|f| caller_is_test || !is_test(f))
        .filter(|f| {
            p.facts
                .get(*f)
                .is_some_and(|ff| ff.package == facts.package)
        })
        .collect();
    let mut r = Resolver {
        p,
        rel,
        facts,
        file: file_id(rel),
        package,
        bound: HashMap::new(),
        dots: Vec::new(),
        ids: HashMap::new(),
        edges: BTreeMap::new(),
        refs: BTreeSet::new(),
    };
    let mut imports_files = BTreeSet::new();

    for i in &facts.imports {
        let path = i.specifier.as_str();
        let alias = i.names.first().map_or("", |n| n.local.as_str());
        let target = p.modules.resolve(path);
        let (bound, package_name) = match &target {
            GoTarget::Package(folder) => {
                // A folder no build holds is no package to import.
                let files: Vec<&str> = p
                    .modules
                    .files_in(folder)
                    .into_iter()
                    .filter(|f| !is_test(f) && !outside_builds(f) && p.facts.contains_key(*f))
                    .collect();
                for f in &files {
                    r.edge("imports", r.file.clone(), file_id(f), i.line, "go.import");
                    imports_files.insert(f.to_string());
                }
                // The package is the files whose clause most of them declare.
                let name = package_name(&files, folder, p.facts);
                let members = files
                    .into_iter()
                    .filter(|f| p.facts[*f].package == name)
                    .collect();
                (Bound::Package(members), name)
            }
            GoTarget::External(_) => {
                r.reference("import", path.to_string(), i.line, "external");
                (Bound::External, None)
            }
            GoTarget::Unresolved => {
                r.reference("import", path.to_string(), i.line, "unresolved");
                (Bound::Unresolved, None)
            }
        };
        match alias {
            "_" => {}
            "." => {
                if let Bound::Package(files) = bound {
                    r.dots.push(files);
                }
            }
            "" => {
                let local = package_name.unwrap_or_else(|| default_name(path).to_string());
                r.bound.entry(local).or_insert(bound);
            }
            alias => {
                r.bound.entry(alias.to_string()).or_insert(bound);
            }
        }
    }

    for c in &facts.calls {
        let src = r.call_source(c);
        match r.resolve_call(c) {
            Outcome::Edge { file, symbol, rule } => {
                let dst = r.symbol_id_of(&file, symbol);
                r.edge("calls", src, dst, c.line, rule);
            }
            Outcome::Ref { status } => r.reference("call", call_text(c), c.line, status),
            Outcome::Nothing => {}
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
                inferred: rule == "go.call.method",
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

/// The package clause most of `files` (the non-test files of `folder`) declare: of
/// two as common, the one named like the folder, else the first in path order. A
/// `//go:build ignore` generator in `package main` does not rename the package.
fn package_name(
    files: &[&str],
    folder: &str,
    facts: &BTreeMap<String, FileFacts>,
) -> Option<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for f in files {
        let Some(name) = facts[*f].package.as_deref() else {
            continue;
        };
        match counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    let most = counts.iter().map(|(_, count)| *count).max()?;
    let top: Vec<&str> = counts
        .iter()
        .filter(|(_, count)| *count == most)
        .map(|(name, _)| *name)
        .collect();
    let base = folder.rsplit('/').next().unwrap_or(folder);
    let name = top.iter().find(|n| **n == base).unwrap_or(&top[0]);
    Some(name.to_string())
}

/// The name an import without an alias binds when its package clause is unknown: the
/// last segment after dropping a final `vN` segment, without a leading `go-` and a
/// trailing `-go`, `.vN` or `.go`.
fn default_name(path: &str) -> &str {
    let mut segments: Vec<&str> = path.split('/').collect();
    if segments.len() > 1 && segments.last().is_some_and(|s| is_version(s)) {
        segments.pop();
    }
    let last = segments.last().copied().unwrap_or(path);
    let last = last.strip_prefix("go-").unwrap_or(last);
    if let Some(stem) = last
        .strip_suffix("-go")
        .or_else(|| last.strip_suffix(".go"))
    {
        return stem;
    }
    match last.rsplit_once('.') {
        Some((stem, version)) if is_version(version) => stem,
        _ => last,
    }
}

/// `v` followed by one or more digits.
fn is_version(s: &str) -> bool {
    s.strip_prefix('v')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `receiver.callee`, or `callee` without a receiver.
fn call_text(c: &CallFact) -> String {
    match &c.receiver {
        Some(r) => format!("{r}.{}", c.callee),
        None => c.callee.clone(),
    }
}

/// The leading identifier of an expression: `s` for `s.Store`.
fn root_ident(expr: &str) -> &str {
    let end = expr
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(expr.len());
    &expr[..end]
}

/// The receiver variable a method signature names: `s` in `func (s *Server) M()`;
/// `None` for `func (Server) M()` or `func (_ T) M()`.
fn receiver_var(sig: &str) -> Option<&str> {
    let inner = sig.strip_prefix("func")?.trim_start().strip_prefix('(')?;
    let inner = inner[..inner.find(')')?].trim();
    let var = root_ident(inner);
    let rest = inner[var.len()..].trim();
    (!var.is_empty() && var != "_" && !rest.is_empty()).then_some(var)
}

struct Resolver<'a, 'p> {
    p: &'a GoProject<'p>,
    rel: &'a str,
    facts: &'a FileFacts,
    file: String,
    /// The files of the caller's package, the caller included.
    package: Vec<&'a str>,
    /// Import local name to what it names; the first import of a name wins.
    bound: HashMap<String, Bound<'a>>,
    /// The files of each dot-imported package of the worktree, in import order.
    dots: Vec<Vec<&'a str>>,
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

    /// The symbol that holds a call, or the file at top level.
    fn call_source(&mut self, c: &CallFact) -> String {
        let held = c
            .owner
            .as_ref()
            .and_then(|(owner, name)| self.facts.holder(owner, name, c.line));
        match held {
            Some(i) => self.symbol_id_of(self.rel, i),
            None => self.file.clone(),
        }
    }

    /// `(file, symbol index)` of every symbol of `files` that passes `keep`.
    fn find(&self, files: &[&str], keep: impl Fn(&SymbolFact) -> bool) -> Vec<(String, usize)> {
        files
            .iter()
            .filter_map(|f| Some((*f, self.p.facts.get(*f)?)))
            .flat_map(|(f, facts)| {
                facts
                    .symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| keep(s))
                    .map(move |(i, _)| (f.to_string(), i))
            })
            .collect()
    }

    /// A call of the function `name` or a conversion to the type `name` in `files`:
    /// `Some` when the package declares the name.
    fn in_package(&self, files: &[&str], name: &str, rule: &'static str) -> Option<Outcome> {
        let functions = self.find(files, |s| {
            s.kind == SymbolKind::Function && s.owner.is_empty() && s.name == name
        });
        match &functions[..] {
            [(file, symbol)] => {
                return Some(Outcome::Edge {
                    file: file.clone(),
                    symbol: *symbol,
                    rule,
                });
            }
            [] => {}
            _ => return Some(UNRESOLVED),
        }
        let types = self.find(files, |s| {
            matches!(s.kind, SymbolKind::Class | SymbolKind::Interface)
                && s.owner.is_empty()
                && s.name == name
        });
        (!types.is_empty()).then_some(Outcome::Nothing)
    }

    /// The receiver variable of the method that holds `c`, with the method's type.
    fn receiver_of(&self, c: &CallFact) -> Option<(&str, &str)> {
        let (owner, name) = c.owner.as_ref()?;
        let s = &self.facts.symbols[self.facts.holder(owner, name, c.line)?];
        if s.kind != SymbolKind::Method {
            return None;
        }
        Some((receiver_var(&s.sig)?, s.owner.as_str()))
    }

    fn resolve_call(&self, c: &CallFact) -> Outcome {
        let callee = c.callee.as_str();
        let Some(receiver) = c.receiver.as_deref() else {
            // `F()` or `T(x)`.
            if let Some(out) = self.in_package(&self.package, callee, "go.call.local") {
                return out;
            }
            for files in &self.dots {
                if let Some(out) = self.in_package(files, callee, "go.call.package") {
                    return out;
                }
            }
            if BUILTINS.contains(&callee) || PREDECLARED_TYPES.contains(&callee) {
                return Outcome::Nothing;
            }
            return UNRESOLVED;
        };

        let root = root_ident(receiver);
        let receiver_var = self.receiver_of(c);
        let is_receiver = receiver_var.is_some_and(|(var, _)| var == root);

        // `r.M()` on the receiver of the enclosing method.
        if let Some((var, owner)) = receiver_var
            && var == receiver
        {
            let methods = self.find(&self.package, |s| {
                s.kind == SymbolKind::Method && s.owner == owner && s.name == callee
            });
            if let [(file, symbol)] = &methods[..] {
                return Outcome::Edge {
                    file: file.clone(),
                    symbol: *symbol,
                    rule: "go.call.receiver",
                };
            }
            return self.by_method_name(callee);
        }

        if !is_receiver && let Some(bound) = self.bound.get(root) {
            match bound {
                // `p.F()` or `p.T(x)`.
                Bound::Package(files) if root == receiver => {
                    return self
                        .in_package(files, callee, "go.call.package")
                        .unwrap_or(UNRESOLVED);
                }
                Bound::Unresolved if root == receiver => return UNRESOLVED,
                Bound::External => return EXTERNAL,
                _ => {}
            }
        }
        self.by_method_name(callee)
    }

    /// The single concrete Method named `name` in the repository's Go code, when an
    /// interface of the project declares that name (spec §6.2: an interface call is
    /// inferred when one implementation exists). Test files and `package main` files
    /// count only in the caller's folder: elsewhere they are another package's tests
    /// or another binary.
    fn by_method_name(&self, name: &str) -> Outcome {
        if !self.p.methods.declared(name) {
            return UNRESOLVED;
        }
        let (dir, _) = split(self.rel);
        let mut candidates = self.p.methods.named(name).iter().filter(|(f, _)| {
            let home = split(f).0 == dir;
            let main = self
                .p
                .facts
                .get(f)
                .is_some_and(|ff| ff.package.as_deref() == Some("main"));
            home || !(is_test(f) || main)
        });
        match (candidates.next(), candidates.next()) {
            (Some((file, symbol)), None) => Outcome::Edge {
                file: file.clone(),
                symbol: *symbol,
                rule: "go.call.method",
            },
            _ => UNRESOLVED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_drop_versions_and_go_affixes() {
        for (path, name) in [
            ("net/http", "http"),
            ("github.com/go-chi/chi/v5", "chi"),
            ("gopkg.in/yaml.v3", "yaml"),
            ("github.com/mattn/go-sqlite3", "sqlite3"),
            ("github.com/x/client-go", "client"),
            ("github.com/x/thing.go", "thing"),
            ("v2", "v2"),
        ] {
            assert_eq!(default_name(path), name, "{path}");
        }
    }

    #[test]
    fn receiver_variables_come_from_the_signature() {
        assert_eq!(receiver_var("func (s *Server) M()"), Some("s"));
        assert_eq!(receiver_var("func (m Memory) M()"), Some("m"));
        assert_eq!(receiver_var("func (t *Tree[K, V]) M()"), Some("t"));
        assert_eq!(receiver_var("func (Server) M()"), None);
        assert_eq!(receiver_var("func (*Server) M()"), None);
        assert_eq!(receiver_var("func (_ T) M()"), None);
        assert_eq!(receiver_var("func F()"), None);
    }
}
