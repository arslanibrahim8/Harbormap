//! Extracts the facts of one Python file (`.py` or `.pyi`) with tree-sitter.
//!
//! The queries in `queries/` find the candidate nodes; the code here reads names,
//! decorators, bodies and annotations from them. The `Query` set is compiled once and
//! each thread keeps one `Parser`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

use crate::facts::{
    AttrFact, CallFact, FileFacts, HeritageFact, ImportFact, ImportKind, ImportedName, LabelFact,
    ParseStatus, SymbolFact, SymbolKind, WebFacts,
};
use crate::hash::{body_hash, empty_body_hash, hash_parts};

const SYMBOLS_SCM: &str = include_str!("queries/symbols.scm");
const IMPORTS_SCM: &str = include_str!("queries/imports.scm");
const CALLS_SCM: &str = include_str!("queries/calls.scm");

/// Receiver text is cut to this many characters.
const RECEIVER_MAX: usize = 80;

struct Queries {
    language: Language,
    symbols: Query,
    imports: Query,
    calls: Query,
}

fn queries() -> &'static Queries {
    static QUERIES: OnceLock<Queries> = OnceLock::new();
    QUERIES.get_or_init(|| {
        let language: Language = tree_sitter_python::LANGUAGE.into();
        let compile = |scm: &str| {
            Query::new(&language, scm)
                .unwrap_or_else(|e| panic!("the bundled Python query compiles: {e}"))
        };
        Queries {
            symbols: compile(SYMBOLS_SCM),
            imports: compile(IMPORTS_SCM),
            calls: compile(CALLS_SCM),
            language,
        }
    })
}

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new(Parser::new());
}

/// Extracts the facts of `src`. A file with syntax errors still yields the facts that
/// parsed, with status `ParsedWithErrors`. A grammar the pinned core refuses to load
/// (`set_language` fails) yields no facts and `ParsedWithErrors`. A bundled query that
/// does not compile is a bug and panics.
pub fn extract(src: &str) -> FileFacts {
    let q = queries();
    let tree = PARSER.with(|p| {
        let mut parser = p.borrow_mut();
        parser.set_language(&q.language).ok()?;
        parser.parse(src, None)
    });
    let Some(tree) = tree else {
        return FileFacts::empty(ParseStatus::ParsedWithErrors);
    };
    let root = tree.root_node();

    let mut ex = Extractor {
        src: src.as_bytes(),
        symbols: Vec::new(),
    };
    let found = ex.collect_symbols(q, root);
    let imports = ex.collect_imports(q, root);
    let calls = ex.collect_calls(q, root);

    FileFacts {
        status: if root.has_error() {
            ParseStatus::ParsedWithErrors
        } else {
            ParseStatus::Parsed
        },
        symbols: ex.symbols.iter().map(|s| s.fact.clone()).collect(),
        imports,
        exports: Vec::new(),
        calls,
        heritage: found.heritage,
        module_names: Vec::new(),
        web: WebFacts::default(),
        package: None,
        labels: ex.labels(&found.inner),
        attrs: found.attrs,
    }
}

/// A symbol with the byte range it occupies, kept to find the symbol around a call.
struct Sym {
    fact: SymbolFact,
    start: usize,
    end: usize,
}

/// What `collect_symbols` finds besides the symbols themselves.
struct Found<'t> {
    heritage: Vec<HeritageFact>,
    attrs: Vec<AttrFact>,
    /// Inner functions and `name = lambda` assignments, as captured.
    inner: Vec<Node<'t>>,
}

struct Extractor<'s> {
    src: &'s [u8],
    symbols: Vec<Sym>,
}

fn line_of(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

fn end_line_of(node: Node) -> u32 {
    node.end_position().row as u32 + 1
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `definition` with its decorators: the `decorated_definition` around it, or itself.
fn with_decorators(definition: Node) -> Node {
    match definition.parent() {
        Some(p) if p.kind() == "decorated_definition" => p,
        _ => definition,
    }
}

impl<'s> Extractor<'s> {
    fn text(&self, node: Node) -> &'s str {
        node.utf8_text(self.src).unwrap_or("")
    }

    fn field_text(&self, node: Node, field: &str) -> &'s str {
        node.child_by_field_name(field).map_or("", |n| self.text(n))
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
        sig_to: usize,
        body: Option<Node>,
        range: Node,
        exported: bool,
    ) {
        if name.is_empty() {
            return;
        }
        let sig = self.signature(sig_from, sig_to);
        let sig_hash = hash_parts(&[kind.as_str(), owner, name, &sig]);
        let body_hash = body.map_or_else(empty_body_hash, |b| body_hash(b, self.src));
        let (l0, l1) = (line_of(range), end_line_of(range));
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
                default_export: false,
                lines: l1 - l0 + 1,
                branches: count_branches(range),
            },
        });
    }

    /// A function or method; the signature runs from the first decorator to the body.
    fn function(&mut self, node: Node, kind: SymbolKind, owner: &str) {
        let range = with_decorators(node);
        let name = self.field_text(node, "name");
        let body = node.child_by_field_name("body");
        let sig_to = body.map_or(node.end_byte(), |b| b.start_byte());
        let exported = kind == SymbolKind::Function && !name.starts_with('_');
        self.push_symbol(kind, name, owner, range, sig_to, body, range, exported);
    }

    /// Adds the class and returns its name.
    fn class(&mut self, node: Node) -> Option<String> {
        let range = with_decorators(node);
        let name = self.field_text(node, "name");
        if name.is_empty() {
            return None;
        }
        let body = node.child_by_field_name("body");
        let sig_to = body.map_or(node.end_byte(), |b| b.start_byte());
        self.push_symbol(
            SymbolKind::Class,
            name,
            "",
            range,
            sig_to,
            body,
            range,
            !name.starts_with('_'),
        );
        Some(name.to_string())
    }

    /// `name = value` or `name: T = value` at the top of the file. A `lambda` value makes
    /// it a function.
    fn variable(&mut self, assignment: Node) {
        let Some(left) = assignment.child_by_field_name("left") else {
            return;
        };
        let name = self.text(left);
        if name.starts_with('_') {
            return;
        }
        let range = assignment.parent().unwrap_or(assignment);
        let value = assignment.child_by_field_name("right");
        let lambda = value.filter(|v| v.kind() == "lambda");
        let (kind, body) = match lambda {
            Some(l) => (SymbolKind::Function, l.child_by_field_name("body")),
            None => (SymbolKind::Variable, value),
        };
        let sig_to = match lambda {
            Some(l) => body.map_or(l.end_byte(), |b| b.start_byte()),
            None => value.map_or(assignment.end_byte(), |v| v.start_byte()),
        };
        self.push_symbol(kind, name, "", range, sig_to, body, range, true);
    }

    /// Fills `self.symbols` in source order and returns the other facts of the classes.
    fn collect_symbols<'t>(&mut self, q: &Queries, root: Node<'t>) -> Found<'t> {
        let names = q.symbols.capture_names();
        let mut seen: HashSet<(usize, u32)> = HashSet::new();
        let mut methods: Vec<Node> = Vec::new();
        let mut classes: Vec<(Node, String)> = Vec::new();
        let mut inner: Vec<Node> = Vec::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&q.symbols, root, self.src);
        while let Some(m) = matches.next() {
            for cap in m.captures() {
                let node = cap.node;
                if !seen.insert((node.id(), cap.index)) {
                    continue;
                }
                match names[cap.index as usize] {
                    "function" => self.function(node, SymbolKind::Function, ""),
                    "class" => {
                        if let Some(name) = self.class(node) {
                            classes.push((node, name));
                        }
                    }
                    "variable" => self.variable(node),
                    "method" => methods.push(node),
                    "inner" | "lambda" => inner.push(node),
                    _ => {}
                }
            }
        }
        drop(matches);

        for method in methods {
            let class = method.parent().and_then(|c| match c.kind() {
                "decorated_definition" => c.parent(),
                _ => Some(c),
            });
            let class = class.and_then(|body| body.parent());
            let Some((_, owner)) =
                class.and_then(|c| classes.iter().find(|(n, _)| n.id() == c.id()))
            else {
                continue;
            };
            let owner = owner.clone();
            self.function(method, SymbolKind::Method, &owner);
        }
        self.symbols.sort_by_key(|s| s.start);

        let mut heritage = Vec::new();
        let mut attrs = Vec::new();
        for (class, name) in &classes {
            self.class_facts(*class, name, &mut heritage, &mut attrs);
        }
        heritage.sort_by_key(|h: &HeritageFact| h.line);
        attrs.sort_by_key(|(start, _): &(usize, AttrFact)| *start);
        Found {
            heritage,
            attrs: attrs.into_iter().map(|(_, a)| a).collect(),
            inner,
        }
    }

    /// The heritage and attribute types of one top-level class.
    fn class_facts(
        &self,
        class: Node,
        name: &str,
        heritage: &mut Vec<HeritageFact>,
        attrs: &mut Vec<(usize, AttrFact)>,
    ) {
        if let Some(bases) = class.child_by_field_name("superclasses") {
            let mut cursor = bases.walk();
            for base in bases.named_children(&mut cursor) {
                if matches!(base.kind(), "identifier" | "attribute") {
                    heritage.push(HeritageFact {
                        class: name.to_string(),
                        extends: collapse(self.text(base)),
                        line: line_of(base),
                    });
                }
            }
        }
        let Some(body) = class.child_by_field_name("body") else {
            return;
        };
        let mut cursor = body.walk();
        for statement in body.named_children(&mut cursor) {
            match statement.kind() {
                "expression_statement" => {
                    let assignment = statement
                        .named_child(0)
                        .filter(|a| a.kind() == "assignment");
                    let Some(assignment) = assignment else {
                        continue;
                    };
                    let left = assignment.child_by_field_name("left");
                    let ty = assignment.child_by_field_name("type");
                    if let (Some(left), Some(ty)) = (left, ty)
                        && left.kind() == "identifier"
                        && let Some(type_name) = self.type_of(ty)
                    {
                        attrs.push((
                            assignment.start_byte(),
                            AttrFact {
                                class: name.to_string(),
                                attr: self.text(left).to_string(),
                                type_name,
                                line: line_of(assignment),
                            },
                        ));
                    }
                }
                "function_definition" => self.method_attrs(statement, name, attrs),
                "decorated_definition" => {
                    if let Some(def) = statement
                        .child_by_field_name("definition")
                        .filter(|d| d.kind() == "function_definition")
                    {
                        self.method_attrs(def, name, attrs);
                    }
                }
                _ => {}
            }
        }
    }

    /// The attribute types a method states for `self`.
    fn method_attrs(&self, method: Node, class: &str, attrs: &mut Vec<(usize, AttrFact)>) {
        let Some(body) = method.child_by_field_name("body") else {
            return;
        };
        let params = method
            .child_by_field_name("parameters")
            .map(|p| self.annotated_parameters(p))
            .unwrap_or_default();
        let mut stack = vec![body];
        while let Some(node) = stack.pop() {
            if node.kind() == "assignment"
                && let Some(type_name) = self.self_attr_type(node, &params)
            {
                let attr = self.field_text(node.child_by_field_name("left").unwrap(), "attribute");
                attrs.push((
                    node.start_byte(),
                    AttrFact {
                        class: class.to_string(),
                        attr: attr.to_string(),
                        type_name,
                        line: line_of(node),
                    },
                ));
            }
            if matches!(
                node.kind(),
                "function_definition" | "class_definition" | "lambda"
            ) {
                continue; // `self` there is a closure, not this method's own state
            }
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                stack.push(child);
            }
        }
    }

    /// The type stated for `self.x` by an assignment `a`, if any.
    fn self_attr_type(&self, a: Node, params: &[(&'s str, String)]) -> Option<String> {
        let left = a.child_by_field_name("left")?;
        if left.kind() != "attribute"
            || left.child_by_field_name("object").map(|o| self.text(o)) != Some("self")
        {
            return None;
        }
        if let Some(ty) = a.child_by_field_name("type") {
            return self.type_of(ty);
        }
        let right = a.child_by_field_name("right")?;
        match right.kind() {
            "call" => {
                let function = right.child_by_field_name("function")?;
                matches!(function.kind(), "identifier" | "attribute")
                    .then(|| collapse(self.text(function)))
            }
            "identifier" => {
                let name = self.text(right);
                params
                    .iter()
                    .find(|(p, _)| *p == name)
                    .map(|(_, ty)| ty.clone())
            }
            _ => None,
        }
    }

    /// `(name, type)` of each parameter whose annotation names a usable type.
    fn annotated_parameters(&self, parameters: Node) -> Vec<(&'s str, String)> {
        let mut out = Vec::new();
        let mut cursor = parameters.walk();
        for p in parameters.named_children(&mut cursor) {
            let name = match p.kind() {
                "typed_parameter" => p.named_child(0).filter(|n| n.kind() == "identifier"),
                "typed_default_parameter" => p.child_by_field_name("name"),
                _ => None,
            };
            let ty = p.child_by_field_name("type").and_then(|t| self.type_of(t));
            if let (Some(name), Some(ty)) = (name, ty) {
                out.push((self.text(name), ty));
            }
        }
        out
    }

    /// The class an annotation names: an identifier or attribute, also inside
    /// `Optional[T]`, `T | None` and `None | T`. Anything else is `None`.
    fn type_of(&self, node: Node) -> Option<String> {
        match node.kind() {
            "type" => self.type_of(node.named_child(0)?),
            "identifier" | "attribute" | "member_type" => Some(collapse(self.text(node))),
            "generic_type" => {
                let head = node.named_child(0)?;
                let params = node
                    .named_children(&mut node.walk())
                    .find(|c| c.kind() == "type_parameter")?;
                let args: Vec<Node> = params.named_children(&mut params.walk()).collect();
                self.optional_inner(self.text(head), &args)
            }
            "subscript" => {
                let value = node.child_by_field_name("value")?;
                let args: Vec<Node> = node
                    .children_by_field_name("subscript", &mut node.walk())
                    .collect();
                self.optional_inner(self.text(value), &args)
            }
            "binary_operator" | "union_type" => {
                if node.kind() == "binary_operator"
                    && node.child_by_field_name("operator").map(|o| o.kind()) != Some("|")
                {
                    return None;
                }
                let parts: Vec<Node> = node.named_children(&mut node.walk()).collect();
                let [a, b] = parts[..] else {
                    return None;
                };
                match (a.kind(), b.kind()) {
                    ("none", _) => self.type_of(b),
                    (_, "none") => self.type_of(a),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The inner type of `Optional[T]`; `None` for any other generic.
    fn optional_inner(&self, head: &str, args: &[Node]) -> Option<String> {
        let is_optional = head == "Optional" || head.ends_with(".Optional");
        match args {
            [only] if is_optional => self.type_of(*only),
            _ => None,
        }
    }

    /// The labels for the captured inner functions and lambda assignments: those inside
    /// the body of a function or method, owned by the symbol around them.
    fn labels(&self, inner: &[Node]) -> Vec<LabelFact> {
        let mut out: Vec<(usize, LabelFact)> = Vec::new();
        for &node in inner {
            let (name, range) = match node.kind() {
                "function_definition" => (self.field_text(node, "name"), with_decorators(node)),
                _ => (self.field_text(node, "left"), node.parent().unwrap_or(node)),
            };
            if name.is_empty() || !inside_function(node) {
                continue;
            }
            out.push((
                range.start_byte(),
                LabelFact {
                    owner: self.enclosing(node.start_byte(), false),
                    name: name.to_string(),
                    l0: line_of(range),
                    l1: end_line_of(range),
                },
            ));
        }
        out.sort_by_key(|(start, _)| *start);
        out.into_iter().map(|(_, l)| l).collect()
    }

    fn collect_imports(&self, q: &Queries, root: Node) -> Vec<ImportFact> {
        let names = q.imports.capture_names();
        let mut found: Vec<(usize, ImportFact)> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&q.imports, root, self.src);
        while let Some(m) = matches.next() {
            for cap in m.captures() {
                let node = cap.node;
                if !seen.insert(node.id()) {
                    continue;
                }
                match names[cap.index as usize] {
                    "import" => self.import(node, &mut found),
                    "from" => self.import_from(node, &mut found),
                    _ => {}
                }
            }
        }
        drop(matches);
        found.sort_by_key(|(start, _)| *start);
        found.into_iter().map(|(_, i)| i).collect()
    }

    /// `import a.b, c as d`: one fact per name; the name bound is the first segment, or
    /// the alias.
    fn import(&self, node: Node, out: &mut Vec<(usize, ImportFact)>) {
        let mut cursor = node.walk();
        for part in node.children_by_field_name("name", &mut cursor) {
            let (dotted, local) = match part.kind() {
                "aliased_import" => {
                    let Some(name) = part.child_by_field_name("name") else {
                        continue;
                    };
                    (name, self.field_text(part, "alias").to_string())
                }
                "dotted_name" => {
                    let text = self.text(part);
                    (
                        part,
                        text.split('.').next().unwrap_or(text).trim().to_string(),
                    )
                }
                _ => continue,
            };
            out.push((
                part.start_byte(),
                ImportFact {
                    specifier: collapse(self.text(dotted)).replace(' ', ""),
                    kind: ImportKind::Static,
                    names: vec![ImportedName {
                        imported: "*".into(),
                        local,
                    }],
                    type_only: false,
                    line: line_of(part),
                },
            ));
        }
    }

    /// `from M import n as k, ...` and `from M import *`; `M` as written.
    fn import_from(&self, node: Node, out: &mut Vec<(usize, ImportFact)>) {
        let Some(module) = node.child_by_field_name("module_name") else {
            return;
        };
        let mut names = Vec::new();
        let mut cursor = node.walk();
        for part in node.children_by_field_name("name", &mut cursor) {
            match part.kind() {
                "dotted_name" => names.push(ImportedName {
                    imported: self.text(part).to_string(),
                    local: self.text(part).to_string(),
                }),
                "aliased_import" => names.push(ImportedName {
                    imported: self.field_text(part, "name").to_string(),
                    local: self.field_text(part, "alias").to_string(),
                }),
                _ => {}
            }
        }
        let mut cursor = node.walk();
        if node
            .children(&mut cursor)
            .any(|c| c.kind() == "wildcard_import")
        {
            names.push(ImportedName {
                imported: "*".into(),
                local: "*".into(),
            });
        }
        if names.is_empty() {
            return;
        }
        out.push((
            node.start_byte(),
            ImportFact {
                specifier: collapse(self.text(module)).replace(' ', ""),
                kind: ImportKind::Static,
                names,
                type_only: false,
                line: line_of(node),
            },
        ));
    }

    fn collect_calls(&self, q: &Queries, root: Node) -> Vec<CallFact> {
        let names = q.calls.capture_names();
        let mut found: Vec<(usize, usize, CallFact)> = Vec::new();
        let mut seen: HashSet<usize> = HashSet::new();

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&q.calls, root, self.src);
        while let Some(m) = matches.next() {
            let mut site: Option<Node> = None;
            let mut callee: Option<Node> = None;
            let mut receiver: Option<Node> = None;
            for cap in m.captures() {
                match names[cap.index as usize] {
                    "call" => site = Some(cap.node),
                    "callee" => callee = Some(cap.node),
                    "receiver" => receiver = Some(cap.node),
                    _ => {}
                }
            }
            let (Some(node), Some(callee)) = (site, callee) else {
                continue;
            };
            if !seen.insert(node.id()) {
                continue;
            }
            let args = node
                .child_by_field_name("arguments")
                .map_or(0, |a| self.count_args(a));
            found.push((
                node.start_byte(),
                node.end_byte(),
                CallFact {
                    owner: self.call_owner(node),
                    callee: self.text(callee).to_string(),
                    receiver: receiver
                        .map(|r| collapse(self.text(r)).chars().take(RECEIVER_MAX).collect()),
                    args,
                    jsx: false,
                    line: line_of(callee),
                },
            ));
        }
        // Source order; of two calls starting together the outer one comes first.
        found.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
        found.into_iter().map(|(_, _, c)| c).collect()
    }

    fn count_args(&self, arguments: Node) -> u32 {
        if arguments.kind() != "argument_list" {
            return 1; // a lone generator expression
        }
        let mut cursor = arguments.walk();
        arguments
            .named_children(&mut cursor)
            .filter(|c| c.kind() != "comment")
            .count() as u32
    }

    /// The owner of a call: the innermost symbol around it, but for a call in a decorator
    /// the symbol around the decorated definition.
    fn call_owner(&self, call: Node) -> Option<(String, String)> {
        let mut up = call.parent();
        while let Some(p) = up {
            if p.kind() == "decorator" {
                let start = p.parent().map_or(p.start_byte(), |d| d.start_byte());
                return self.enclosing(start, true);
            }
            up = p.parent();
        }
        self.enclosing(call.start_byte(), false)
    }

    /// `(owner, name)` of the innermost symbol that contains `pos`. With `strict`, a
    /// symbol that starts at `pos` does not count.
    fn enclosing(&self, pos: usize, strict: bool) -> Option<(String, String)> {
        self.symbols
            .iter()
            .filter(|s| (s.start < pos || (!strict && s.start == pos)) && pos < s.end)
            .min_by_key(|s| s.end - s.start)
            .map(|s| (s.fact.owner.clone(), s.fact.name.clone()))
    }
}

/// Whether `node` sits inside the body of some `function_definition`.
fn inside_function(node: Node) -> bool {
    let mut up = node.parent();
    while let Some(p) = up {
        if p.kind() == "function_definition" {
            return true;
        }
        up = p.parent();
    }
    false
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

/// `if`, `elif`, loops, `except`, `?:`-style conditionals, `and`/`or`, `case` and
/// comprehension `if`s inside `node`.
fn count_branches(node: Node) -> u32 {
    let mut count = u32::from(matches!(
        node.kind(),
        "if_statement"
            | "elif_clause"
            | "for_statement"
            | "while_statement"
            | "except_clause"
            | "conditional_expression"
            | "boolean_operator"
            | "case_clause"
            | "if_clause"
    ));
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_branches(child);
    }
    count
}
