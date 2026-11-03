//! Extracts the facts of one Go file with tree-sitter.
//!
//! The queries in `queries/` find the candidate nodes; the code here reads names,
//! receivers and bodies from them. Each `Query` is compiled once and each thread keeps
//! one `Parser`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

use crate::facts::{
    CallFact, FileFacts, ImportFact, ImportKind, ImportedName, LabelFact, ParseStatus, SymbolFact,
    SymbolKind,
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

fn build_queries() -> Queries {
    let language: Language = tree_sitter_go::LANGUAGE.into();
    let compile = |scm: &str| {
        Query::new(&language, scm).unwrap_or_else(|e| panic!("the bundled Go query compiles: {e}"))
    };
    Queries {
        symbols: compile(SYMBOLS_SCM),
        imports: compile(IMPORTS_SCM),
        calls: compile(CALLS_SCM),
        language,
    }
}

fn queries() -> &'static Queries {
    static GO: OnceLock<Queries> = OnceLock::new();
    GO.get_or_init(build_queries)
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
    let literals = ex.collect_symbols(q, root);
    let (package, imports) = ex.collect_imports(q, root);
    let calls = ex.collect_calls(q, root);
    let labels = ex.labels(literals);

    FileFacts {
        status: if root.has_error() {
            ParseStatus::ParsedWithErrors
        } else {
            ParseStatus::Parsed
        },
        symbols: ex.symbols.into_iter().map(|s| s.fact).collect(),
        imports,
        exports: Vec::new(),
        calls,
        heritage: Vec::new(),
        package,
        labels,
        attrs: Vec::new(),
    }
}

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

/// Go exports the names that start with an upper-case letter.
fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
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

    fn push_symbol(
        &mut self,
        kind: SymbolKind,
        name: &str,
        owner: &str,
        node: Node,
        body: Option<Node>,
    ) {
        if name.is_empty() {
            return;
        }
        let sig_end = body.map_or(node.end_byte(), |b| b.start_byte());
        let sig = self.signature(node, sig_end);
        let sig_hash = hash_parts(&[kind.as_str(), owner, name, &sig]);
        let body_hash = body.map_or_else(empty_body_hash, |b| body_hash(b, self.src));
        let (l0, l1) = (line_of(node), node.end_position().row as u32 + 1);
        self.symbols.push(Sym {
            start: node.start_byte(),
            end: node.end_byte(),
            fact: SymbolFact {
                kind,
                name: name.to_string(),
                owner: owner.to_string(),
                sig,
                sig_hash,
                body_hash,
                l0,
                l1,
                exported: is_exported(name),
                default_export: false,
                lines: l1 - l0 + 1,
                branches: count_branches(node),
            },
        });
    }

    /// Fills `self.symbols` in source order and returns the function literals.
    fn collect_symbols<'t>(&mut self, q: &Queries, root: Node<'t>) -> Vec<Node<'t>> {
        let names = q.symbols.capture_names();
        let mut seen: HashSet<usize> = HashSet::new();
        let mut literals = Vec::new();

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
                    "method" => self.method(node),
                    "type" => self.type_decl(node),
                    "literal" => literals.push(node),
                    _ => {}
                }
            }
        }
        drop(matches);
        self.symbols.sort_by_key(|s| s.start);
        literals
    }

    fn function(&mut self, node: Node) {
        let name = self.field_text(node, "name");
        self.push_symbol(
            SymbolKind::Function,
            name,
            "",
            node,
            node.child_by_field_name("body"),
        );
    }

    fn method(&mut self, node: Node) {
        let owner = node
            .child_by_field_name("receiver")
            .and_then(|r| self.receiver_type(r));
        let Some(owner) = owner else {
            return;
        };
        let name = self.field_text(node, "name");
        self.push_symbol(
            SymbolKind::Method,
            name,
            &owner,
            node,
            node.child_by_field_name("body"),
        );
    }

    /// The type name of a receiver, without `*` and type parameters.
    fn receiver_type(&self, receiver: Node) -> Option<String> {
        let mut cursor = receiver.walk();
        let param = receiver
            .named_children(&mut cursor)
            .find(|c| c.kind() == "parameter_declaration")?;
        let mut ty = param.child_by_field_name("type")?;
        loop {
            match ty.kind() {
                "pointer_type" | "parenthesized_type" => ty = ty.named_child(0)?,
                "generic_type" => ty = ty.child_by_field_name("type")?,
                "type_identifier" => return Some(self.text(ty).to_string()),
                "qualified_type" => return Some(self.field_text(ty, "name").to_string()),
                _ => return None,
            }
        }
    }

    /// A `type_spec` or `type_alias` at the top level of the file: an interface, or a
    /// class for every other type. The whole spec is the signature and the body. Each
    /// method an interface declares is a method owned by the interface, with the spec
    /// as its signature and no body.
    fn type_decl(&mut self, node: Node) {
        let top_level = node
            .parent()
            .and_then(|d| d.parent())
            .is_some_and(|f| f.kind() == "source_file");
        if !top_level {
            return;
        }
        let name = self.field_text(node, "name");
        let interface = node
            .child_by_field_name("type")
            .filter(|t| node.kind() == "type_spec" && t.kind() == "interface_type");
        let kind = if interface.is_some() {
            SymbolKind::Interface
        } else {
            SymbolKind::Class
        };
        if name.is_empty() {
            return;
        }
        self.push_symbol(kind, name, "", node, None);
        if let Some(last) = self.symbols.last_mut() {
            last.fact.body_hash = body_hash(node, self.src);
        }
        if let Some(interface) = interface {
            let mut cursor = interface.walk();
            for spec in interface.named_children(&mut cursor) {
                if spec.kind() == "method_elem" {
                    let method = self.field_text(spec, "name");
                    self.push_symbol(SymbolKind::Method, method, name, spec, None);
                }
            }
        }
    }

    /// The package clause and the imports, in source order.
    fn collect_imports(&self, q: &Queries, root: Node) -> (Option<String>, Vec<ImportFact>) {
        let names = q.imports.capture_names();
        let mut package = None;
        let mut imports: Vec<(usize, ImportFact)> = Vec::new();
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
                    "package" => {
                        package.get_or_insert_with(|| self.text(node).to_string());
                    }
                    "import" => {
                        if let Some(fact) = self.import(node) {
                            imports.push((node.start_byte(), fact));
                        }
                    }
                    _ => {}
                }
            }
        }
        drop(matches);
        imports.sort_by_key(|(start, _)| *start);
        (package, imports.into_iter().map(|(_, i)| i).collect())
    }

    fn import(&self, node: Node) -> Option<ImportFact> {
        let path = self.text(node.child_by_field_name("path")?);
        let specifier = path.get(1..path.len().checked_sub(1)?)?.to_string();
        Some(ImportFact {
            specifier,
            kind: ImportKind::Static,
            names: vec![ImportedName {
                imported: "*".into(),
                local: self.field_text(node, "name").into(),
            }],
            type_only: false,
            line: line_of(node),
        })
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
            let args = node.child_by_field_name("arguments").map_or(0, |a| {
                let mut cursor = a.walk();
                a.named_children(&mut cursor)
                    .filter(|c| c.kind() != "comment")
                    .count() as u32
            });
            found.push((
                node.start_byte(),
                node.end_byte(),
                CallFact {
                    owner: self.enclosing(node.start_byte()),
                    callee: self.text(callee).to_string(),
                    receiver: receiver
                        .map(|r| collapse(self.text(r)).chars().take(RECEIVER_MAX).collect()),
                    args,
                    jsx: false,
                    line: line_of(callee),
                },
            ));
        }
        drop(matches);
        // Source order; of two calls starting together the outer one comes first.
        found.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
        found.into_iter().map(|(_, _, c)| c).collect()
    }

    /// `(owner, name)` of the innermost symbol that contains `pos`.
    fn enclosing(&self, pos: usize) -> Option<(String, String)> {
        self.symbols
            .iter()
            .filter(|s| s.start <= pos && pos < s.end)
            .min_by_key(|s| s.end - s.start)
            .map(|s| (s.fact.owner.clone(), s.fact.name.clone()))
    }

    /// The labels of the function literals that are bound to a name, in source order.
    fn labels(&self, literals: Vec<Node>) -> Vec<LabelFact> {
        let mut found: Vec<(usize, LabelFact)> = literals
            .into_iter()
            .filter_map(|lit| {
                let name = self.literal_name(lit)?;
                Some((
                    lit.start_byte(),
                    LabelFact {
                        owner: self.enclosing(lit.start_byte()),
                        name,
                        l0: line_of(lit),
                        l1: lit.end_position().row as u32 + 1,
                    },
                ))
            })
            .collect();
        found.sort_by_key(|(start, _)| *start);
        found.into_iter().map(|(_, l)| l).collect()
    }

    /// The name a function literal is given: the left identifier of the declaration or
    /// assignment it is the value of, directly or as an argument of the call that is;
    /// or the key of the keyed element it is the value of.
    fn literal_name(&self, lit: Node) -> Option<String> {
        let parent = lit.parent()?;
        match parent.kind() {
            "expression_list" => self.bound_name(lit),
            "argument_list" => {
                let call = parent.parent().filter(|c| c.kind() == "call_expression")?;
                self.bound_name(call)
            }
            "literal_element" => {
                let keyed = parent.parent().filter(|k| k.kind() == "keyed_element")?;
                let value = keyed.child_by_field_name("value")?;
                if value.id() != parent.id() {
                    return None;
                }
                let key = keyed.child_by_field_name("key")?;
                let key = if key.kind() == "literal_element" {
                    key.named_child(0)?
                } else {
                    key
                };
                matches!(key.kind(), "identifier" | "field_identifier")
                    .then(|| self.text(key).to_string())
            }
            _ => None,
        }
    }

    /// The identifier at the same position on the left of a declaration or assignment
    /// whose right-hand value is `value`.
    fn bound_name(&self, value: Node) -> Option<String> {
        let list = value.parent().filter(|p| p.kind() == "expression_list")?;
        let decl = list.parent()?;
        let mut cursor = list.walk();
        let position = list
            .named_children(&mut cursor)
            .filter(|c| c.kind() != "comment")
            .position(|c| c.id() == value.id())?;
        let target = match decl.kind() {
            "short_var_declaration" | "assignment_statement" => {
                if decl.child_by_field_name("right")?.id() != list.id() {
                    return None;
                }
                let left = decl.child_by_field_name("left")?;
                let mut cursor = left.walk();
                left.named_children(&mut cursor)
                    .filter(|c| c.kind() != "comment")
                    .nth(position)?
            }
            "var_spec" => {
                if decl.child_by_field_name("value")?.id() != list.id() {
                    return None;
                }
                let mut cursor = decl.walk();
                decl.children_by_field_name("name", &mut cursor)
                    .nth(position)?
            }
            _ => return None,
        };
        (target.kind() == "identifier").then(|| self.text(target).to_string())
    }
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

/// `if`, `for`, `case` clauses, `&&` and `||` inside `node`.
fn count_branches(node: Node) -> u32 {
    let mut count = match node.kind() {
        "if_statement" | "for_statement" | "expression_case" | "type_case"
        | "communication_case" => 1,
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
