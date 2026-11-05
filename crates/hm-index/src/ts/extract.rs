//! Extracts the facts of one TypeScript, TSX or JavaScript file with tree-sitter.
//!
//! The queries in `queries/` find the candidate nodes; the code here reads names,
//! export wrappers and bodies from them. Each `Query` is compiled once per language
//! and each thread keeps one `Parser`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

use crate::facts::{
    CallFact, ExportFact, FileFacts, HeritageFact, ImportFact, ImportKind, ImportedName,
    ParseStatus, SymbolFact, SymbolKind,
};
use crate::hash::{body_hash, empty_body_hash, hash_parts};
use crate::lang::Lang;

use super::web;

const SYMBOLS_SCM: &str = include_str!("queries/symbols.scm");
const IMPORTS_SCM: &str = include_str!("queries/imports.scm");
const CALLS_SCM: &str = include_str!("queries/calls.scm");

/// Receiver text is cut to this many characters.
const RECEIVER_MAX: usize = 80;

/// Node kinds that are function values when they are the initialiser of a `const`.
const FUNCTION_VALUES: [&str; 3] = [
    "arrow_function",
    "function_expression",
    "generator_function",
];

struct Queries {
    language: Language,
    symbols: Query,
    imports: Query,
    calls: Query,
}

/// The part of a query file that applies to `lang`; see the `;;@` markers in the files.
pub(crate) fn query_source(scm: &str, lang: Lang) -> String {
    let ts = matches!(lang, Lang::TypeScript | Lang::Tsx);
    let jsx = matches!(lang, Lang::Tsx | Lang::JavaScript);
    let mut on = true;
    let mut out = String::new();
    for line in scm.lines() {
        if let Some(section) = line.strip_prefix(";;@") {
            on = match section.trim() {
                "ts" => ts,
                "jsx" => jsx,
                _ => true,
            };
        } else if on {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn build_queries(lang: Lang) -> Option<Queries> {
    let language: Language = match lang {
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        _ => return None,
    };
    let compile = |scm: &str| {
        Query::new(&language, &query_source(scm, lang))
            .unwrap_or_else(|e| panic!("the bundled {lang:?} query compiles: {e}"))
    };
    Some(Queries {
        symbols: compile(SYMBOLS_SCM),
        imports: compile(IMPORTS_SCM),
        calls: compile(CALLS_SCM),
        language,
    })
}

fn queries(lang: Lang) -> Option<&'static Queries> {
    static TS: OnceLock<Option<Queries>> = OnceLock::new();
    static TSX: OnceLock<Option<Queries>> = OnceLock::new();
    static JS: OnceLock<Option<Queries>> = OnceLock::new();
    let cell = match lang {
        Lang::TypeScript => &TS,
        Lang::Tsx => &TSX,
        Lang::JavaScript => &JS,
        _ => return None,
    };
    cell.get_or_init(|| build_queries(lang)).as_ref()
}

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Extracts the facts of `src`. A file with syntax errors still yields the facts that
/// parsed, with status `ParsedWithErrors`. A language L1 does not parse, or a grammar the
/// pinned core refuses to load (`set_language` fails), yields no facts and
/// `ParsedWithErrors`. A bundled query that does not compile is a bug and panics.
pub fn extract(src: &str, lang: Lang) -> FileFacts {
    let failed = || FileFacts::empty(ParseStatus::ParsedWithErrors);
    let Some(q) = queries(lang) else {
        return failed();
    };
    let tree = PARSER.with(|p| {
        let mut parser = p.borrow_mut();
        parser.set_language(&q.language).ok()?;
        parser.parse(src, None)
    });
    let Some(tree) = tree else {
        return failed();
    };
    let root = tree.root_node();
    let bytes = src.as_bytes();

    let mut ex = Extractor {
        src: bytes,
        symbols: Vec::new(),
    };
    let heritage = ex.collect_symbols(q, root);
    ex.push_callbacks(web::route_callbacks(root, bytes, lang));
    let (imports, mut exports) = ex.collect_imports(q, root);
    ex.mark_locally_exported(&exports);
    let calls = ex.collect_calls(q, root);
    let spans: Vec<web::Span> = ex
        .symbols
        .iter()
        .map(|s| web::Span {
            start: s.start,
            end: s.end,
            owner: s.fact.owner.clone(),
            name: s.fact.name.clone(),
        })
        .collect();
    let web = web::extract_web(root, bytes, lang, &spans);
    let module_names = ex.module_names(root);
    exports.sort_by_key(|(start, _)| *start);

    FileFacts {
        status: if root.has_error() {
            ParseStatus::ParsedWithErrors
        } else {
            ParseStatus::Parsed
        },
        symbols: ex.symbols.into_iter().map(|s| s.fact).collect(),
        imports: imports.into_iter().map(|(_, i)| i).collect(),
        exports: exports.into_iter().map(|(_, e)| e).collect(),
        calls,
        heritage,
        module_names,
        web,
        package: None,
        labels: Vec::new(),
        attrs: Vec::new(),
    }
}

/// Imports and exports, each with the start byte of its statement.
type Imports = Vec<(usize, ImportFact)>;
type Exports = Vec<(usize, ExportFact)>;

/// A symbol with the byte range it occupies, kept to find the symbol around a call.
struct Sym {
    fact: SymbolFact,
    start: usize,
    end: usize,
}

struct Extractor<'s> {
    src: &'s [u8],
    symbols: Vec<Sym>,
}

fn line_of(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `node` has the anonymous child token `token` (`default`, `type`, ...).
fn has_token(node: Node, token: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|c| !c.is_named() && c.kind() == token)
}

impl<'s> Extractor<'s> {
    fn text(&self, node: Node) -> &'s str {
        node.utf8_text(self.src).unwrap_or("")
    }

    fn field_text(&self, node: Node, field: &str) -> &'s str {
        node.child_by_field_name(field).map_or("", |n| self.text(n))
    }

    /// The text of a string literal without its quotes; `None` for anything else.
    fn string_value(&self, node: Node) -> Option<String> {
        if node.kind() != "string" {
            return None;
        }
        let t = self.text(node);
        t.get(1..t.len().checked_sub(1)?).map(str::to_string)
    }

    /// The source from `from`'s start to byte `to`, without comments, whitespace
    /// collapsed.
    fn signature(&self, from: Node, to: usize) -> String {
        let mut comments = Vec::new();
        collect_comments(from, to, &mut comments);
        let mut out = String::new();
        let mut at = from.start_byte();
        for (s, e) in comments {
            out.push_str(&String::from_utf8_lossy(&self.src[at..s]));
            out.push(' ');
            at = e;
        }
        out.push_str(&String::from_utf8_lossy(&self.src[at..to.max(at)]));
        collapse(&out)
    }

    #[allow(clippy::too_many_arguments)]
    fn push_symbol(
        &mut self,
        kind: SymbolKind,
        name: &str,
        owner: &str,
        sig_from: Node,
        body: Option<Node>,
        range: Node,
        exported: bool,
        default_export: bool,
    ) {
        if name.is_empty() {
            return;
        }
        let sig_end = body.map_or(sig_from.end_byte(), |b| b.start_byte());
        let sig = self.signature(sig_from, sig_end);
        let sig_hash = hash_parts(&[kind.as_str(), owner, name, &sig]);
        let body_hash = body.map_or_else(empty_body_hash, |b| body_hash(b, self.src));
        let (l0, l1) = (line_of(range), range.end_position().row as u32 + 1);
        self.symbols.push(Sym {
            start: range.start_byte(),
            end: range.end_byte(),
            fact: SymbolFact {
                kind,
                name: name.to_string(),
                owner: owner.to_string(),
                sig,
                sig_hash,
                body_hash,
                l0,
                l1,
                exported,
                default_export,
                lines: l1 - l0 + 1,
                branches: count_branches(range),
            },
        });
    }

    /// Fills `self.symbols` in source order and returns the heritage facts.
    fn collect_symbols(&mut self, q: &Queries, root: Node) -> Vec<HeritageFact> {
        let names = q.symbols.capture_names();
        let mut seen: HashSet<usize> = HashSet::new();
        let mut methods: Vec<Node> = Vec::new();
        let mut heritages: Vec<Node> = Vec::new();
        let mut classes: HashMap<usize, String> = HashMap::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&q.symbols, root, self.src);
        while let Some(m) = matches.next() {
            for cap in m.captures() {
                let node = cap.node;
                if !seen.insert(node.id()) {
                    continue;
                }
                match names[cap.index as usize] {
                    "function" => self.function(node),
                    "commonjs" => self.commonjs(node),
                    "declarator" => self.declarator(node),
                    "class" => {
                        if let Some(name) = self.class(node) {
                            classes.insert(node.id(), name);
                        }
                    }
                    "interface" => self.interface(node),
                    "method" => methods.push(node),
                    "heritage" => heritages.push(node),
                    _ => {}
                }
            }
        }
        drop(matches);

        for method in methods {
            let class = method.parent().and_then(|body| body.parent());
            let Some(owner) = class.and_then(|c| classes.get(&c.id())) else {
                continue;
            };
            let owner = owner.clone();
            let name = self.field_text(method, "name");
            self.push_symbol(
                SymbolKind::Method,
                name,
                &owner,
                method,
                method.child_by_field_name("body"),
                method,
                false,
                false,
            );
        }
        self.symbols.sort_by_key(|s| s.start);

        let mut heritage: Vec<HeritageFact> = heritages
            .into_iter()
            .filter_map(|h| {
                let class = h.parent()?;
                let class_name = classes.get(&class.id())?;
                let target = if let Some(clause) = first_child_of_kind(h, "extends_clause") {
                    clause.child_by_field_name("value")?
                } else {
                    // The JS grammar puts the expression directly in the heritage; in TS a
                    // lone `implements` clause is not an `extends`.
                    h.named_child(0)
                        .filter(|c| c.kind() != "implements_clause")?
                };
                Some(HeritageFact {
                    class: class_name.clone(),
                    extends: collapse(self.text(target)),
                    line: line_of(h),
                })
            })
            .collect();
        heritage.sort_by_key(|h| h.line);
        heritage
    }

    /// Adds the inline route handlers as symbols, so a call inside one has it as owner.
    fn push_callbacks(&mut self, callbacks: Vec<web::Callback>) {
        self.symbols.extend(callbacks.into_iter().map(|c| Sym {
            fact: c.fact,
            start: c.start,
            end: c.end,
        }));
        self.symbols.sort_by_key(|s| s.start);
    }

    /// The statement-level wrapper of a declaration: `(export_statement, exported,
    /// default)`, or the node itself.
    fn wrapper<'t>(&self, decl: Node<'t>) -> (Node<'t>, bool, bool) {
        match decl.parent() {
            Some(p) if p.kind() == "export_statement" => (p, true, has_token(p, "default")),
            _ => (decl, false, false),
        }
    }

    fn function(&mut self, node: Node) {
        let (range, exported, default_export) = self.wrapper(node);
        let name = match self.field_text(node, "name") {
            "" => "default",
            n => n,
        };
        self.push_symbol(
            SymbolKind::Function,
            name,
            "",
            node,
            node.child_by_field_name("body"),
            range,
            exported,
            default_export,
        );
    }

    /// `module.exports = function f() {}`: an exported default function.
    fn commonjs(&mut self, node: Node) {
        let Some(statement) = node.parent().and_then(|a| a.parent()) else {
            return;
        };
        let name = match self.field_text(node, "name") {
            "" => "default",
            n => n,
        };
        self.push_symbol(
            SymbolKind::Function,
            name,
            "",
            statement,
            node.child_by_field_name("body"),
            statement,
            true,
            true,
        );
    }

    fn declarator(&mut self, node: Node) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        if name_node.kind() != "identifier" {
            return;
        }
        let Some(declaration) = node.parent() else {
            return;
        };
        let (_, exported, _) = self.wrapper(declaration);
        let value = node.child_by_field_name("value");
        let is_function = value.is_some_and(|v| FUNCTION_VALUES.contains(&v.kind()));
        if !is_function && !exported {
            return;
        }
        // A statement with several declarators gives each its own range.
        let mut cursor = declaration.walk();
        let single = declaration
            .children(&mut cursor)
            .filter(|c| c.kind() == "variable_declarator")
            .count()
            == 1;
        let (sig_from, range) = if single {
            (declaration, self.wrapper(declaration).0)
        } else {
            (node, node)
        };
        let body = if is_function {
            value.and_then(|v| v.child_by_field_name("body"))
        } else {
            value
        };
        let kind = if is_function {
            SymbolKind::Function
        } else {
            SymbolKind::Variable
        };
        self.push_symbol(
            kind,
            self.text(name_node),
            "",
            sig_from,
            body,
            range,
            exported,
            false,
        );
    }

    /// Adds the class and returns its name.
    fn class(&mut self, node: Node) -> Option<String> {
        let (range, exported, default_export) = self.wrapper(node);
        let name = self.field_text(node, "name");
        if name.is_empty() {
            return None;
        }
        self.push_symbol(
            SymbolKind::Class,
            name,
            "",
            node,
            node.child_by_field_name("body"),
            range,
            exported,
            default_export,
        );
        Some(name.to_string())
    }

    /// Interfaces and type aliases: the whole declaration is the signature.
    fn interface(&mut self, node: Node) {
        let (range, exported, _) = self.wrapper(node);
        let name = self.field_text(node, "name");
        if name.is_empty() {
            return;
        }
        self.push_symbol(
            SymbolKind::Interface,
            name,
            "",
            node,
            None,
            range,
            exported,
            false,
        );
        if let Some(last) = self.symbols.last_mut() {
            last.fact.body_hash = body_hash(node, self.src);
        }
    }

    /// Every name a top-level statement binds that is not a top-level symbol, sorted
    /// and without duplicates; see `FileFacts::module_names`.
    fn module_names(&self, root: Node) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = root.walk();
        for statement in root.named_children(&mut cursor) {
            self.statement_names(statement, &mut names);
        }
        names.retain(|n| {
            !self
                .symbols
                .iter()
                .any(|s| s.fact.owner.is_empty() && s.fact.name == *n)
        });
        names.sort();
        names.dedup();
        names
    }

    /// The names a top-level statement binds, looking through `export` and `declare`.
    fn statement_names(&self, node: Node, out: &mut Vec<String>) {
        let mut cursor = node.walk();
        match node.kind() {
            "lexical_declaration" | "variable_declaration" => {
                for d in node.named_children(&mut cursor) {
                    if let Some(name) = d.child_by_field_name("name") {
                        self.pattern_names(name, out);
                    }
                }
            }
            "export_statement" => {
                if let Some(declaration) = node.child_by_field_name("declaration") {
                    self.statement_names(declaration, out);
                }
            }
            "ambient_declaration" | "expression_statement" => {
                for child in node.named_children(&mut cursor) {
                    self.statement_names(child, out);
                }
            }
            "enum_declaration"
            | "function_signature"
            | "function_declaration"
            | "generator_function_declaration"
            | "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    out.push(self.text(name).to_string());
                }
            }
            // `namespace A.B {}` binds `A`; `module "x" {}` binds nothing.
            "internal_module" | "module" => {
                let first = node.child_by_field_name("name").and_then(|name| {
                    let mut at = name;
                    while at.kind() == "nested_identifier" {
                        at = at.named_child(0)?;
                    }
                    Some(at)
                });
                if let Some(name) = first.filter(|n| n.kind() == "identifier") {
                    out.push(self.text(name).to_string());
                }
            }
            // `import x = require('y')` and `import x = A.B`.
            "import_statement" | "import_alias" => {
                if let Some(clause) = node
                    .named_children(&mut cursor)
                    .find(|c| c.kind() == "import_require_clause")
                    .or_else(|| (node.kind() == "import_alias").then_some(node))
                {
                    let mut inner = clause.walk();
                    let name = clause
                        .named_children(&mut inner)
                        .find(|c| c.kind() == "identifier");
                    if let Some(name) = name {
                        out.push(self.text(name).to_string());
                    }
                }
            }
            _ => {}
        }
    }

    /// Every name a binding pattern introduces: identifiers, object and array
    /// destructuring with defaults and rest.
    fn pattern_names(&self, node: Node, out: &mut Vec<String>) {
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                out.push(self.text(node).to_string());
            }
            "pair_pattern" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.pattern_names(value, out);
                }
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                if let Some(left) = node.child_by_field_name("left") {
                    self.pattern_names(left, out);
                }
            }
            "object_pattern" | "array_pattern" | "rest_pattern" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.pattern_names(child, out);
                }
            }
            _ => {}
        }
    }

    /// `function a() {}` followed by `export { a }` makes `a` exported.
    fn mark_locally_exported(&mut self, exports: &[(usize, ExportFact)]) {
        for (_, e) in exports {
            let (Some(local), None) = (&e.local, &e.from) else {
                continue;
            };
            for s in self.symbols.iter_mut().filter(|s| s.fact.owner.is_empty()) {
                if s.fact.name == *local {
                    s.fact.exported = true;
                    s.fact.default_export |= e.name == "default";
                }
            }
        }
    }

    /// Imports and exports with their start bytes.
    fn collect_imports(&self, q: &Queries, root: Node) -> (Imports, Exports) {
        let names = q.imports.capture_names();
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&q.imports, root, self.src);
        while let Some(m) = matches.next() {
            let mut main: Option<(&str, Node)> = None;
            let mut source: Option<Node> = None;
            for cap in m.captures() {
                match names[cap.index as usize] {
                    "source" => source = Some(cap.node),
                    n @ ("import" | "export" | "dynamic" | "require") => main = Some((n, cap.node)),
                    _ => {}
                }
            }
            let Some((what, node)) = main else { continue };
            if !seen.insert(node.id()) {
                continue;
            }
            let spec = source.and_then(|s| self.string_value(s));
            match what {
                "import" => {
                    if let Some(spec) = spec {
                        imports.push((node.start_byte(), self.static_import(node, spec)));
                    }
                }
                "export" => self.export(node, &mut imports, &mut exports),
                "dynamic" => {
                    if let Some(spec) = spec {
                        imports.push((
                            node.start_byte(),
                            ImportFact {
                                specifier: spec,
                                kind: ImportKind::Dynamic,
                                names: Vec::new(),
                                type_only: false,
                                line: line_of(node),
                            },
                        ));
                    }
                }
                _ => {
                    if let Some(spec) = spec {
                        imports.push((
                            node.start_byte(),
                            ImportFact {
                                specifier: spec,
                                kind: ImportKind::Require,
                                names: self.require_names(node),
                                type_only: false,
                                line: line_of(node),
                            },
                        ));
                    }
                }
            }
        }
        drop(matches);
        imports.sort_by_key(|(start, _)| *start);
        (imports, exports)
    }

    fn static_import(&self, node: Node, specifier: String) -> ImportFact {
        let mut names = Vec::new();
        let mut all_type_only = true;
        let mut cursor = node.walk();
        if let Some(clause) = node
            .children(&mut cursor)
            .find(|c| c.kind() == "import_clause")
        {
            let mut cursor = clause.walk();
            for part in clause.children(&mut cursor) {
                match part.kind() {
                    "identifier" => {
                        all_type_only = false;
                        names.push(ImportedName {
                            imported: "default".into(),
                            local: self.text(part).into(),
                        });
                    }
                    "namespace_import" => {
                        all_type_only = false;
                        if let Some(id) = first_child_of_kind(part, "identifier") {
                            names.push(ImportedName {
                                imported: "*".into(),
                                local: self.text(id).into(),
                            });
                        }
                    }
                    "named_imports" => {
                        let mut cursor = part.walk();
                        for spec in part.children(&mut cursor) {
                            if spec.kind() != "import_specifier" {
                                continue;
                            }
                            all_type_only &= has_token(spec, "type");
                            let imported = self.field_text(spec, "name");
                            let local = match self.field_text(spec, "alias") {
                                "" => imported,
                                alias => alias,
                            };
                            names.push(ImportedName {
                                imported: imported.into(),
                                local: local.into(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        } else {
            all_type_only = false;
        }
        ImportFact {
            specifier,
            kind: ImportKind::Static,
            type_only: has_token(node, "type") || (!names.is_empty() && all_type_only),
            names,
            line: line_of(node),
        }
    }

    /// The names bound by `const { a, b: c } = require("s")` or `const m = require("s")`.
    fn require_names(&self, call: Node) -> Vec<ImportedName> {
        let Some(declarator) = call
            .parent()
            .filter(|p| p.kind() == "variable_declarator")
            .filter(|p| p.child_by_field_name("value").map(|v| v.id()) == Some(call.id()))
        else {
            return Vec::new();
        };
        let Some(target) = declarator.child_by_field_name("name") else {
            return Vec::new();
        };
        match target.kind() {
            "identifier" => vec![ImportedName {
                imported: "*".into(),
                local: self.text(target).into(),
            }],
            "object_pattern" => {
                let mut out = Vec::new();
                let mut cursor = target.walk();
                for part in target.children(&mut cursor) {
                    match part.kind() {
                        "shorthand_property_identifier_pattern" => out.push(ImportedName {
                            imported: self.text(part).into(),
                            local: self.text(part).into(),
                        }),
                        "pair_pattern" => {
                            let value = part.child_by_field_name("value");
                            if let Some(v) = value.filter(|v| v.kind() == "identifier") {
                                out.push(ImportedName {
                                    imported: self.field_text(part, "key").into(),
                                    local: self.text(v).into(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                out
            }
            _ => Vec::new(),
        }
    }

    /// One `export` statement gives export facts and, with a source, a re-export import.
    fn export(&self, node: Node, imports: &mut Imports, exports: &mut Exports) {
        let start = node.start_byte();
        let line = line_of(node);
        let default = has_token(node, "default");
        let from = node
            .child_by_field_name("source")
            .and_then(|s| self.string_value(s));
        let mut push = |name: &str, local: Option<&str>, star: bool| {
            exports.push((
                start,
                ExportFact {
                    name: name.into(),
                    local: local.map(str::to_string),
                    from: from.clone(),
                    star,
                    line,
                },
            ));
        };
        let mut reexport_names: Vec<ImportedName> = Vec::new();

        if let Some(decl) = node.child_by_field_name("declaration") {
            let mut declared: Vec<&str> = Vec::new();
            match decl.kind() {
                "function_declaration"
                | "generator_function_declaration"
                | "class_declaration"
                | "abstract_class_declaration"
                | "interface_declaration"
                | "type_alias_declaration" => declared.push(self.field_text(decl, "name")),
                "lexical_declaration" | "variable_declaration" => {
                    let mut cursor = decl.walk();
                    for d in decl.children(&mut cursor) {
                        let name = d.child_by_field_name("name");
                        if let Some(n) = name.filter(|n| n.kind() == "identifier") {
                            declared.push(self.text(n));
                        }
                    }
                }
                _ => {}
            }
            for name in declared.into_iter().filter(|n| !n.is_empty()) {
                push(if default { "default" } else { name }, Some(name), false);
            }
        } else if let Some(value) = node.child_by_field_name("value") {
            let local = match value.kind() {
                "identifier" => Some(self.text(value)),
                "function_expression" | "generator_function" => {
                    Some(match self.field_text(value, "name") {
                        "" => "default",
                        n => n,
                    })
                }
                "arrow_function" => Some("default"),
                _ => None,
            };
            push("default", local, false);
        } else if let Some(clause) = first_child_of_kind(node, "export_clause") {
            let mut cursor = clause.walk();
            for spec in clause.children(&mut cursor) {
                if spec.kind() != "export_specifier" {
                    continue;
                }
                let local = self.field_text(spec, "name");
                let exported = match self.field_text(spec, "alias") {
                    "" => local,
                    alias => alias,
                };
                push(exported, Some(local), false);
                reexport_names.push(ImportedName {
                    imported: local.into(),
                    local: exported.into(),
                });
            }
        } else if let Some(ns) = first_child_of_kind(node, "namespace_export") {
            let name = ns.named_child(0).map_or("", |n| self.text(n));
            push(name, Some("*"), false);
            reexport_names.push(ImportedName {
                imported: "*".into(),
                local: name.into(),
            });
        } else if from.is_some() {
            push("*", None, true);
        }

        if let Some(spec) = from.clone() {
            imports.push((
                start,
                ImportFact {
                    specifier: spec,
                    kind: ImportKind::ReExport,
                    names: reexport_names,
                    type_only: has_token(node, "type"),
                    line,
                },
            ));
        }
    }

    fn collect_calls(&self, q: &Queries, root: Node) -> Vec<CallFact> {
        let names = q.calls.capture_names();
        let mut found: Vec<(usize, usize, CallFact)> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&q.calls, root, self.src);
        while let Some(m) = matches.next() {
            let mut site: Option<(bool, Node)> = None;
            let mut callee: Option<Node> = None;
            let mut receiver: Option<Node> = None;
            for cap in m.captures() {
                match names[cap.index as usize] {
                    "call" => site = Some((false, cap.node)),
                    "jsx" => site = Some((true, cap.node)),
                    "callee" => callee = Some(cap.node),
                    "receiver" => receiver = Some(cap.node),
                    _ => {}
                }
            }
            let (Some((jsx, node)), Some(callee)) = (site, callee) else {
                continue;
            };
            if !seen.insert(node.id()) {
                continue;
            }
            let callee_text = self.text(callee);
            if jsx
                && receiver.is_none()
                && !callee_text.chars().next().is_some_and(char::is_uppercase)
            {
                continue; // `<div/>` is an element, not a component
            }
            let args = if jsx {
                0
            } else {
                node.child_by_field_name("arguments")
                    .map_or(0, |a| self.count_args(a))
            };
            let owner = self.enclosing(node.start_byte());
            found.push((
                node.start_byte(),
                node.end_byte(),
                CallFact {
                    owner,
                    callee: callee_text.to_string(),
                    receiver: receiver
                        .map(|r| collapse(self.text(r)).chars().take(RECEIVER_MAX).collect()),
                    args,
                    jsx,
                    line: line_of(callee),
                },
            ));
        }
        // Source order; of two calls starting together the outer one comes first.
        found.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
        found.into_iter().map(|(_, _, c)| c).collect()
    }

    fn count_args(&self, arguments: Node) -> u32 {
        if arguments.kind() != "arguments" {
            return 1; // a tagged template
        }
        let mut cursor = arguments.walk();
        arguments
            .named_children(&mut cursor)
            .filter(|c| c.kind() != "comment")
            .count() as u32
    }

    /// `(owner, name)` of the innermost symbol that contains `pos`.
    fn enclosing(&self, pos: usize) -> Option<(String, String)> {
        self.symbols
            .iter()
            .filter(|s| s.start <= pos && pos < s.end)
            .min_by_key(|s| s.end - s.start)
            .map(|s| (s.fact.owner.clone(), s.fact.name.clone()))
    }
}

fn first_child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

/// Byte ranges of the comments inside `node` that start before `limit`.
fn collect_comments(node: Node, limit: usize, out: &mut Vec<(usize, usize)>) {
    if node.start_byte() >= limit {
        return;
    }
    if node.kind() == "comment" {
        out.push((node.start_byte(), node.end_byte().min(limit)));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comments(child, limit, out);
    }
}

/// `if`, loops, `case`, `catch`, `?:`, `&&` and `||` inside `node`.
pub(crate) fn count_branches(node: Node) -> u32 {
    let mut count = match node.kind() {
        "if_statement" | "for_statement" | "for_in_statement" | "while_statement"
        | "do_statement" | "switch_case" | "catch_clause" | "ternary_expression" => 1,
        "binary_expression" => u32::from(matches!(
            node.child_by_field_name("operator").map(|o| o.kind()),
            Some("&&" | "||")
        )),
        _ => 0,
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_branches(child);
    }
    count
}
