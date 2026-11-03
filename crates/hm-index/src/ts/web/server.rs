//! Server-side web facts of one file: Express route calls and bindings, NestJS decorators
//! and constructor fields, dynamic-code gaps and environment reads (Learn spec §6.3).
//!
//! `server.scm` finds the candidate nodes; the rules live here. Whether an argument is an
//! inline route handler that became a synthetic symbol is decided in one place
//! (`callback_of`), for both the symbols and the facts, so the two never disagree.

use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

use super::{Callback, Span, compile, owner_at};
use crate::facts::{
    ArgFact, BindingFact, CtorFieldFact, DecoratorFact, EnvFact, GapFact, Owner, RouteCallFact,
    SymbolFact, SymbolKind, WebFacts,
};
use crate::hash::{body_hash, hash_parts};
use crate::lang::Lang;

const SERVER_SCM: &str = include_str!("queries/server.scm");

/// Methods of a route call. The HTTP verbs and `all` define routes, `use` mounts
/// middleware and routers, `setGlobalPrefix` is NestJS's.
const ROUTE_METHODS: [&str; 10] = [
    "get",
    "post",
    "put",
    "patch",
    "delete",
    "head",
    "options",
    "all",
    "use",
    "setGlobalPrefix",
];

/// The route methods whose last inline function becomes a symbol.
const HANDLER_METHODS: [&str; 8] = [
    "get", "post", "put", "patch", "delete", "head", "options", "all",
];

/// Initialisers that make a binding interesting: Express apps and routers, Nest apps.
const BINDING_INITS: [&str; 4] = ["express", "Router", "express.Router", "NestFactory.create"];

/// Node kinds that are inline functions.
const FUNCTION_KINDS: [&str; 3] = [
    "arrow_function",
    "function_expression",
    "generator_function",
];

fn query(lang: Lang) -> Option<&'static Query> {
    static TS: OnceLock<Option<Query>> = OnceLock::new();
    static TSX: OnceLock<Option<Query>> = OnceLock::new();
    static JS: OnceLock<Option<Query>> = OnceLock::new();
    let cell = match lang {
        Lang::TypeScript => &TS,
        Lang::Tsx => &TSX,
        Lang::JavaScript => &JS,
        _ => return None,
    };
    cell.get_or_init(|| compile(SERVER_SCM, lang)).as_ref()
}

fn text<'s>(node: Node, src: &'s [u8]) -> &'s str {
    node.utf8_text(src).unwrap_or("")
}

fn line_of(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

/// The text without any whitespace, for comparing `import . meta` style spellings.
fn compact(node: Node, src: &[u8]) -> String {
    text(node, src)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// The named children of `node` that are not comments.
fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect()
}

/// The text of a string literal without its quotes, or of a template literal with each
/// `${…}` as `{}`; `None` for any other node.
fn string_text(node: Node, src: &[u8]) -> Option<String> {
    match node.kind() {
        "string" => {
            let t = text(node, src);
            t.get(1..t.len().checked_sub(1)?).map(str::to_string)
        }
        "template_string" => {
            let mut out = String::new();
            let mut cursor = node.walk();
            for part in node.named_children(&mut cursor) {
                match part.kind() {
                    "template_substitution" => out.push_str("{}"),
                    _ => out.push_str(text(part, src)),
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Whether `node` is a string or a template literal without substitutions.
fn is_literal(node: Node) -> bool {
    match node.kind() {
        "string" => true,
        "template_string" => {
            let mut cursor = node.walk();
            !node
                .named_children(&mut cursor)
                .any(|c| c.kind() == "template_substitution")
        }
        _ => false,
    }
}

/// A call `receiver.method(args…)` that can be a route call, with its argument nodes.
struct RouteShape<'t> {
    receiver: &'t str,
    method: &'t str,
    args: Vec<Node<'t>>,
}

/// Reads `R.m(args…)` with `R` an identifier and `m` a route method, when the first
/// argument fits: a string starting with `/` for the verbs, any argument for `use`, any
/// string for `setGlobalPrefix`.
fn route_shape<'t>(call: Node<'t>, src: &'t [u8]) -> Option<RouteShape<'t>> {
    let callee = call.child_by_field_name("function")?;
    if callee.kind() != "member_expression" {
        return None;
    }
    let object = callee.child_by_field_name("object")?;
    let property = callee.child_by_field_name("property")?;
    if object.kind() != "identifier" || property.kind() != "property_identifier" {
        return None;
    }
    let (receiver, method) = (text(object, src), text(property, src));
    if !ROUTE_METHODS.contains(&method) {
        return None;
    }
    let args = named_children(call.child_by_field_name("arguments")?);
    let first = *args.first()?;
    let fits = match method {
        "use" => true,
        "setGlobalPrefix" => matches!(first.kind(), "string" | "template_string"),
        _ => string_text(first, src).is_some_and(|p| p.starts_with('/')),
    };
    fits.then_some(RouteShape {
        receiver,
        method,
        args,
    })
}

/// The inline handler of a route call that becomes a synthetic symbol: the last argument
/// of an HTTP-verb or `all` call that is a statement of its own at the top level of the
/// file. Returns the function and the symbol name (`GET /health`).
fn callback_of<'t>(
    call: Node<'t>,
    shape: &RouteShape<'t>,
    src: &[u8],
) -> Option<(Node<'t>, String)> {
    if !HANDLER_METHODS.contains(&shape.method) {
        return None;
    }
    let statement = call.parent()?;
    // The root is `program`, or an `ERROR` node while the file is broken mid-typing.
    if statement.kind() != "expression_statement" || statement.parent()?.parent().is_some() {
        return None;
    }
    let function = *shape.args.last()?;
    if !FUNCTION_KINDS.contains(&function.kind()) {
        return None;
    }
    let path = string_text(*shape.args.first()?, src)?;
    Some((function, format!("{} {path}", shape.method.to_uppercase())))
}

/// The inline route handlers of the file, as symbols.
pub(crate) fn route_callbacks(root: Node, src: &[u8], _lang: Lang) -> Vec<Callback> {
    let mut cursor = root.walk();
    let mut out = Vec::new();
    for statement in root.named_children(&mut cursor) {
        if statement.kind() != "expression_statement" {
            continue;
        }
        let Some(call) = statement
            .named_child(0)
            .filter(|c| c.kind() == "call_expression")
        else {
            continue;
        };
        let Some(shape) = route_shape(call, src) else {
            continue;
        };
        let Some((function, name)) = callback_of(call, &shape, src) else {
            continue;
        };
        let (l0, l1) = (line_of(function), function.end_position().row as u32 + 1);
        out.push(Callback {
            start: function.start_byte(),
            end: function.end_byte(),
            fact: SymbolFact {
                kind: SymbolKind::Function,
                sig_hash: hash_parts(&[&name]),
                body_hash: body_hash(function, src),
                sig: name.clone(),
                name,
                owner: String::new(),
                l0,
                l1,
                exported: false,
                default_export: false,
                lines: l1 - l0 + 1,
                branches: count_branches(function),
            },
        });
    }
    out
}

/// Decision points in `node`, counted as `extract.rs` counts them for other functions.
fn count_branches(node: Node) -> u32 {
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

/// One argument of a call. `handler` is the inline function that became a symbol, with
/// the symbol's name.
fn arg_fact(node: Node, src: &[u8], handler: Option<&(Node, String)>) -> ArgFact {
    if let Some(s) = string_text(node, src) {
        return ArgFact::Str(s);
    }
    match node.kind() {
        "identifier" => ArgFact::Name(text(node, src).to_string()),
        "member_expression" if is_name_path(node) => ArgFact::Name(compact(node, src)),
        "object" => ArgFact::Object {
            path: object_path(node, src),
        },
        k if FUNCTION_KINDS.contains(&k) => ArgFact::Inline(
            handler
                .filter(|(f, _)| f.id() == node.id())
                .map(|(_, name)| name.clone()),
        ),
        _ => ArgFact::Other,
    }
}

/// Whether `node` is `a.b.c`: identifiers, `this` and property names only.
fn is_name_path(node: Node) -> bool {
    match node.kind() {
        "identifier" | "this" => true,
        "member_expression" => {
            node.child_by_field_name("object").is_some_and(is_name_path)
                && node
                    .child_by_field_name("property")
                    .is_some_and(|p| p.kind() == "property_identifier")
        }
        _ => false,
    }
}

/// The string value of the `path` property of an object literal.
fn object_path(object: Node, src: &[u8]) -> Option<String> {
    named_children(object).into_iter().find_map(|pair| {
        if pair.kind() != "pair" {
            return None;
        }
        let key = pair.child_by_field_name("key")?;
        let key = match key.kind() {
            "property_identifier" => text(key, src).to_string(),
            _ => string_text(key, src)?,
        };
        if key != "path" {
            return None;
        }
        string_text(pair.child_by_field_name("value")?, src)
    })
}

/// The name of a class node, `""` for an anonymous one.
fn class_name<'s>(class: Node, src: &'s [u8]) -> &'s str {
    class
        .child_by_field_name("name")
        .map_or("", |n| text(n, src))
}

fn is_class(node: Node) -> bool {
    matches!(
        node.kind(),
        "class_declaration" | "abstract_class_declaration"
    )
}

/// `(class, member)` of the declaration a decorator is written on, with `""` as class for
/// a decorator on a class itself; `None` for decorators on anything else (fields,
/// parameters) or on an unnamed class.
fn decorator_target(decorator: Node, src: &[u8]) -> Option<(String, String)> {
    let parent = decorator.parent()?;
    match parent.kind() {
        // In a class body the decorators are siblings that come before their member.
        "class_body" => {
            let mut member = decorator.next_named_sibling();
            while member.is_some_and(|m| matches!(m.kind(), "decorator" | "comment")) {
                member = member.and_then(|m| m.next_named_sibling());
            }
            let member = member.filter(|m| m.kind() == "method_definition")?;
            let class = parent.parent()?;
            let name = Some(class_name(class, src)).filter(|n| is_class(class) && !n.is_empty())?;
            let method = text(member.child_by_field_name("name")?, src);
            Some((name.to_string(), method.to_string()))
        }
        kind => {
            // `@Dec export class A {}`: the decorator belongs to the export statement.
            let class = if kind == "export_statement" {
                parent.child_by_field_name("declaration")?
            } else {
                parent
            };
            let name = Some(class_name(class, src)).filter(|n| is_class(class) && !n.is_empty())?;
            Some((String::new(), name.to_string()))
        }
    }
}

fn decorator_fact(node: Node, src: &[u8]) -> Option<DecoratorFact> {
    let target = decorator_target(node, src)?;
    let inner = node.named_child(0)?;
    let (name, args) = if inner.kind() == "call_expression" {
        let callee = inner.child_by_field_name("function")?;
        let args = inner
            .child_by_field_name("arguments")
            .map(named_children)
            .unwrap_or_default();
        (compact(callee, src), args)
    } else {
        (compact(inner, src), Vec::new())
    };
    Some(DecoratorFact {
        target,
        name,
        args: args.into_iter().map(|a| arg_fact(a, src, None)).collect(),
        line: line_of(node),
    })
}

/// The parameter properties of a constructor whose type is a plain name.
fn ctor_fields(ctor: Node, src: &[u8], out: &mut Vec<(usize, CtorFieldFact)>) {
    let Some(class) = ctor.parent().and_then(|body| body.parent()) else {
        return;
    };
    let class = class_name(class, src);
    let Some(params) = ctor.child_by_field_name("parameters") else {
        return;
    };
    for param in named_children(params) {
        if !matches!(param.kind(), "required_parameter" | "optional_parameter") {
            continue;
        }
        let mut cursor = param.walk();
        let is_property = param.children(&mut cursor).any(|c| {
            matches!(
                c.kind(),
                "accessibility_modifier" | "override_modifier" | "readonly"
            )
        });
        if !is_property {
            continue;
        }
        let (Some(pattern), Some(annotation)) = (
            param.child_by_field_name("pattern"),
            param.child_by_field_name("type"),
        ) else {
            continue;
        };
        let Some(ty) = annotation
            .named_child(0)
            .filter(|t| t.kind() == "type_identifier")
        else {
            continue;
        };
        if pattern.kind() != "identifier" || class.is_empty() {
            continue;
        }
        out.push((
            param.start_byte(),
            CtorFieldFact {
                class: class.to_string(),
                field: text(pattern, src).to_string(),
                type_name: text(ty, src).to_string(),
                line: line_of(param),
            },
        ));
    }
}

/// The variable name of `process.env.NAME`, `process.env['NAME']` and
/// `import.meta.env.NAME`.
fn env_name(node: Node, src: &[u8]) -> Option<String> {
    let object = compact(node.child_by_field_name("object")?, src);
    if object != "process.env" && object != "import.meta.env" {
        return None;
    }
    if node.kind() == "subscript_expression" {
        return string_text(node.child_by_field_name("index")?, src);
    }
    Some(text(node.child_by_field_name("property")?, src).to_string())
}

/// The kind of gap a call or `new` is, if it is one.
fn gap_kind(node: Node, callee: Node, src: &[u8]) -> Option<&'static str> {
    if node.kind() == "new_expression" {
        return (text(callee, src) == "Function").then_some("new_function");
    }
    let dynamic = || {
        node.child_by_field_name("arguments")
            .and_then(|a| named_children(a).first().copied())
            .is_some_and(|first| !is_literal(first))
    };
    match (callee.kind(), text(callee, src)) {
        ("identifier", "eval") => Some("eval"),
        ("identifier", "require") => dynamic().then_some("dynamic_require"),
        ("import", _) => dynamic().then_some("dynamic_import"),
        _ => None,
    }
}

pub(crate) fn extract(root: Node, src: &[u8], lang: Lang, spans: &[Span], out: &mut WebFacts) {
    let Some(q) = query(lang) else {
        return;
    };
    let names = q.capture_names();
    let owner = |node: Node| -> Owner { owner_at(spans, node.start_byte()) };

    let mut route_calls = Vec::new();
    let mut bindings = Vec::new();
    let mut decorators = Vec::new();
    let mut ctor_fields_found = Vec::new();
    let mut gaps = Vec::new();
    let mut env = Vec::new();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(q, root, src);
    while let Some(m) = matches.next() {
        let mut callee = None;
        for cap in m.captures() {
            if names[cap.index as usize] == "callee" {
                callee = Some(cap.node);
            }
        }
        for cap in m.captures() {
            let node = cap.node;
            let start = node.start_byte();
            match names[cap.index as usize] {
                "route" => {
                    let Some(shape) = route_shape(node, src) else {
                        continue;
                    };
                    let handler = callback_of(node, &shape, src);
                    route_calls.push((
                        start,
                        RouteCallFact {
                            owner: owner(node),
                            receiver: shape.receiver.to_string(),
                            method: shape.method.to_string(),
                            args: shape
                                .args
                                .iter()
                                .map(|a| arg_fact(*a, src, handler.as_ref()))
                                .collect(),
                            line: line_of(node),
                        },
                    ));
                }
                "binding" => {
                    let (Some(name), Some(value)) = (
                        node.child_by_field_name("name"),
                        node.child_by_field_name("value"),
                    ) else {
                        continue;
                    };
                    let call = if value.kind() == "await_expression" {
                        value.named_child(0)
                    } else {
                        Some(value)
                    };
                    let init = call.and_then(|c| {
                        c.child_by_field_name(if c.kind() == "new_expression" {
                            "constructor"
                        } else {
                            "function"
                        })
                    });
                    let Some(init) = init.map(|i| compact(i, src)) else {
                        continue;
                    };
                    if BINDING_INITS.contains(&init.as_str()) {
                        bindings.push((
                            start,
                            BindingFact {
                                owner: owner(node),
                                name: text(name, src).to_string(),
                                init,
                                line: line_of(node),
                            },
                        ));
                    }
                }
                "decorator" => {
                    if let Some(fact) = decorator_fact(node, src) {
                        decorators.push((start, fact));
                    }
                }
                "ctor" => ctor_fields(node, src, &mut ctor_fields_found),
                "gap_call" | "gap_new" => {
                    let Some(callee) = callee else {
                        continue;
                    };
                    if let Some(kind) = gap_kind(node, callee, src) {
                        gaps.push((
                            start,
                            GapFact {
                                owner: owner(node),
                                kind: kind.to_string(),
                                line: line_of(node),
                            },
                        ));
                    }
                }
                "env_member" | "env_subscript" => {
                    if let Some(name) = env_name(node, src) {
                        env.push((
                            start,
                            EnvFact {
                                owner: owner(node),
                                name,
                                line: line_of(node),
                            },
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    drop(matches);

    out.route_calls = sorted(route_calls);
    out.bindings = sorted(bindings);
    out.decorators = sorted(decorators);
    out.ctor_fields = sorted(ctor_fields_found);
    out.gaps = sorted(gaps);
    out.env = sorted(env);
}

/// The facts in source order.
fn sorted<T>(mut found: Vec<(usize, T)>) -> Vec<T> {
    found.sort_by_key(|(start, _)| *start);
    found.into_iter().map(|(_, fact)| fact).collect()
}
