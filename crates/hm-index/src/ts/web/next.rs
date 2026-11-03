//! Next.js and React facts of one file: directives, forms, action aliases and the
//! middleware matcher (Learn spec §6.3). The query finds the candidate nodes; the code
//! here reads names and values from them.

use std::sync::OnceLock;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

use super::{Span, compile, owner_at};
use crate::facts::{AliasFact, ArgFact, DirectiveFact, FormFact, MatcherFact, WebFacts};
use crate::lang::Lang;

const NEXT_SCM: &str = include_str!("queries/next.scm");

/// The directives that mean something to React and Next.js.
const DIRECTIVES: [&str; 3] = ["use server", "use client", "use cache"];

/// The hooks whose second result is a form action that stands for their first argument.
const ACTION_HOOKS: [&str; 2] = ["useActionState", "useFormState"];

/// Node kinds that are functions; a `const` inside one is local to it.
const FUNCTION_KINDS: [&str; 6] = [
    "function_declaration",
    "generator_function_declaration",
    "function_expression",
    "generator_function",
    "arrow_function",
    "method_definition",
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
    cell.get_or_init(|| compile(NEXT_SCM, lang)).as_ref()
}

fn text<'s>(node: Node, src: &'s [u8]) -> &'s str {
    node.utf8_text(src).unwrap_or("")
}

fn line_of(node: Node) -> u32 {
    node.start_position().row as u32 + 1
}

/// The text of a string literal without its quotes; `None` for anything else, a template
/// literal included.
fn string_value(node: Node, src: &[u8]) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let t = text(node, src);
    t.get(1..t.len().checked_sub(1)?).map(str::to_string)
}

/// `node` without parentheses and without `as` / `satisfies` / `!` wrappers.
fn peel(mut node: Node) -> Node {
    while matches!(
        node.kind(),
        "parenthesized_expression"
            | "as_expression"
            | "satisfies_expression"
            | "non_null_expression"
    ) {
        match node.named_child(0) {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

pub(crate) fn extract(root: Node, src: &[u8], lang: Lang, spans: &[Span], out: &mut WebFacts) {
    let Some(q) = query(lang) else {
        return;
    };
    let names = q.capture_names();
    let mut directives: Vec<(usize, DirectiveFact)> = Vec::new();
    prologue(root, src, spans, None, &mut directives);

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(q, root, src);
    while let Some(m) = matches.next() {
        for cap in m.captures() {
            let node = cap.node;
            match names[cap.index as usize] {
                "function" => function_directives(node, src, spans, &mut directives),
                "declarator" => {
                    if let Some(alias) = alias(node, src, spans) {
                        out.aliases.push(alias);
                    }
                }
                "element" => {
                    if let Some(form) = form(node, src, spans) {
                        out.forms.push(form);
                    }
                }
                _ => {}
            }
        }
    }
    drop(matches);

    directives.sort_by_key(|(start, _)| *start);
    out.directives
        .extend(directives.into_iter().map(|(_, d)| d));
    out.matcher = matcher(root, src);
}

/// The directives in the prologue of `parent` (a `program` or a `statement_block`): the
/// string statements before the first statement that is anything else. Comments between
/// them are skipped.
fn prologue(
    parent: Node,
    src: &[u8],
    spans: &[Span],
    function: Option<&str>,
    out: &mut Vec<(usize, DirectiveFact)>,
) {
    let mut cursor = parent.walk();
    for stmt in parent.named_children(&mut cursor) {
        if matches!(stmt.kind(), "comment" | "hash_bang_line") {
            continue;
        }
        let literal = (stmt.kind() == "expression_statement")
            .then(|| stmt.named_child(0))
            .flatten()
            .and_then(|e| string_value(e, src));
        let Some(value) = literal else {
            break;
        };
        if DIRECTIVES.contains(&value.as_str()) {
            let owner = function.and_then(|_| owner_at(spans, stmt.start_byte()));
            out.push((
                stmt.start_byte(),
                DirectiveFact {
                    owner,
                    function: function.map(str::to_string),
                    text: value,
                    line: line_of(stmt),
                },
            ));
        }
    }
}

/// The directives at the start of the body of function node `f`.
fn function_directives(f: Node, src: &[u8], spans: &[Span], out: &mut Vec<(usize, DirectiveFact)>) {
    let Some(body) = f
        .child_by_field_name("body")
        .filter(|b| b.kind() == "statement_block")
    else {
        return;
    };
    let name = function_name(f, src);
    prologue(body, src, spans, Some(&name), out);
}

/// The name of function node `f`: its own, or the one JavaScript gives it from the
/// variable it is assigned to; `""` for an anonymous function. A property key is not a
/// binding in scope, so `{ run: async () => {} }` stays anonymous.
fn function_name(f: Node, src: &[u8]) -> String {
    if let Some(name) = f.child_by_field_name("name") {
        return text(name, src).to_string();
    }
    f.parent()
        .filter(|p| p.kind() == "variable_declarator")
        .and_then(|p| p.child_by_field_name("name"))
        .filter(|n| n.kind() == "identifier")
        .map_or_else(String::new, |n| text(n, src).to_string())
}

fn inside_function(node: Node) -> bool {
    let mut at = node.parent();
    while let Some(n) = at {
        if FUNCTION_KINDS.contains(&n.kind()) {
            return true;
        }
        at = n.parent();
    }
    false
}

/// `const L = X.bind(…)` or `const [a, L] = useActionState(X, …)` inside a function.
fn alias(decl: Node, src: &[u8], spans: &[Span]) -> Option<AliasFact> {
    if !inside_function(decl) {
        return None;
    }
    let lhs = decl.child_by_field_name("name")?;
    let call = decl.child_by_field_name("value")?;
    let callee = call.child_by_field_name("function")?;
    let (local, target, via) = match lhs.kind() {
        "identifier" if callee.kind() == "member_expression" => {
            let property = callee.child_by_field_name("property")?;
            let object = callee.child_by_field_name("object")?;
            if text(property, src) != "bind" || object.kind() != "identifier" {
                return None;
            }
            (lhs, object, "bind")
        }
        "array_pattern" => {
            let hook = match callee.kind() {
                "identifier" => text(callee, src),
                "member_expression" => text(callee.child_by_field_name("property")?, src),
                _ => return None,
            };
            if !ACTION_HOOKS.contains(&hook) {
                return None;
            }
            let local = pattern_slot(lhs, 1).filter(|n| n.kind() == "identifier")?;
            let first = call
                .child_by_field_name("arguments")?
                .named_child(0)
                .filter(|n| n.kind() == "identifier")?;
            (local, first, "useActionState")
        }
        _ => return None,
    };
    Some(AliasFact {
        owner: owner_at(spans, decl.start_byte()),
        local: text(local, src).to_string(),
        target: text(target, src).to_string(),
        via: via.to_string(),
        line: line_of(decl),
    })
}

/// The element of array pattern `pattern` at `slot`, counting holes: tree-sitter leaves
/// them out of the named children, so the commas are counted instead.
fn pattern_slot(pattern: Node, slot: usize) -> Option<Node> {
    let mut at = 0;
    let mut cursor = pattern.walk();
    for child in pattern.children(&mut cursor) {
        if child.kind() == "," {
            at += 1;
        } else if child.is_named() && child.kind() != "comment" && at == slot {
            return Some(child);
        }
    }
    None
}

/// A `<form>` element, or a `<button>` / `<input>` that has a `formAction`.
fn form(element: Node, src: &[u8], spans: &[Span]) -> Option<FormFact> {
    let tag = text(element.child_by_field_name("name")?, src);
    let (action_attr, method_attr) = if tag == "form" {
        ("action", "method")
    } else {
        ("formAction", "formMethod")
    };
    let mut action = None;
    let mut method = None;
    let mut on_submit = false;
    let mut has_action = false;
    let mut cursor = element.walk();
    for attr in element.named_children(&mut cursor) {
        if attr.kind() != "jsx_attribute" {
            continue;
        }
        let Some(name) = attr.named_child(0) else {
            continue;
        };
        let value = attr.named_child(1);
        match text(name, src) {
            n if n == action_attr => {
                has_action = true;
                action = value.map(|v| attribute_action(v, src));
            }
            n if n == method_attr => {
                method = value
                    .and_then(|v| attribute_string(v, src))
                    .map(|s| s.to_uppercase());
            }
            "onSubmit" => on_submit = true,
            _ => {}
        }
    }
    if tag != "form" && !has_action {
        return None;
    }
    Some(FormFact {
        owner: owner_at(spans, element.start_byte()),
        element: tag.to_string(),
        action,
        method,
        on_submit,
        line: line_of(element),
    })
}

/// The string literal an attribute value is: `"x"` or `{'x'}`.
fn attribute_string(value: Node, src: &[u8]) -> Option<String> {
    if let Some(s) = string_value(value, src) {
        return Some(s);
    }
    (value.kind() == "jsx_expression")
        .then(|| value.named_child(0))
        .flatten()
        .map(peel)
        .and_then(|e| string_value(e, src))
}

/// What a `form` / `formAction` attribute value is, as far as a rule needs to read it.
fn attribute_action(value: Node, src: &[u8]) -> ArgFact {
    if let Some(s) = attribute_string(value, src) {
        return ArgFact::Str(s);
    }
    let expr = (value.kind() == "jsx_expression")
        .then(|| value.named_child(0))
        .flatten()
        .map(peel);
    match expr {
        Some(e) if is_name_path(e) => ArgFact::Name(text(e, src).to_string()),
        _ => ArgFact::Other,
    }
}

/// An identifier or a chain of property reads on one: `save`, `actions.save`.
fn is_name_path(node: Node) -> bool {
    match node.kind() {
        "identifier" => true,
        "member_expression" => {
            let mut cursor = node.walk();
            let optional = node
                .children(&mut cursor)
                .any(|c| c.kind() == "optional_chain");
            !optional
                && node
                    .child_by_field_name("property")
                    .is_some_and(|p| p.kind() == "property_identifier")
                && node.child_by_field_name("object").is_some_and(is_name_path)
        }
        _ => false,
    }
}

/// `export const config = { matcher: … }`.
fn matcher(root: Node, src: &[u8]) -> Option<MatcherFact> {
    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        if stmt.kind() != "export_statement" {
            continue;
        }
        let Some(decl) = stmt
            .child_by_field_name("declaration")
            .filter(|d| d.kind() == "lexical_declaration")
        else {
            continue;
        };
        let mut inner = decl.walk();
        for d in decl.named_children(&mut inner) {
            let named_config = d
                .child_by_field_name("name")
                .is_some_and(|n| text(n, src) == "config");
            let object = d
                .child_by_field_name("value")
                .map(peel)
                .filter(|v| v.kind() == "object");
            let (true, Some(object)) = (named_config, object) else {
                continue;
            };
            let values = matcher_values(object, src);
            if !values.is_empty() {
                return Some(MatcherFact {
                    values,
                    line: line_of(stmt),
                });
            }
        }
    }
    None
}

/// The strings of the `matcher` property of config object `object`, in source order.
fn matcher_values(object: Node, src: &[u8]) -> Vec<String> {
    let mut cursor = object.walk();
    for pair in object.named_children(&mut cursor) {
        let is_matcher = pair.kind() == "pair"
            && pair.child_by_field_name("key").is_some_and(|k| {
                string_value(k, src).unwrap_or_else(|| text(k, src).to_string()) == "matcher"
            });
        let Some(value) = pair
            .child_by_field_name("value")
            .map(peel)
            .filter(|_| is_matcher)
        else {
            continue;
        };
        let mut values = Vec::new();
        match value.kind() {
            "array" => {
                let mut items = value.walk();
                for item in value.named_children(&mut items) {
                    matcher_item(peel(item), src, &mut values);
                }
            }
            _ => matcher_item(value, src, &mut values),
        }
        return values;
    }
    Vec::new()
}

/// A matcher entry: a string, or an object with a `source` string.
fn matcher_item(item: Node, src: &[u8], out: &mut Vec<String>) {
    if let Some(s) = string_value(item, src) {
        out.push(s);
    } else if item.kind() == "object" {
        let mut cursor = item.walk();
        for pair in item.named_children(&mut cursor) {
            let is_source = pair.kind() == "pair"
                && pair.child_by_field_name("key").is_some_and(|k| {
                    string_value(k, src).unwrap_or_else(|| text(k, src).to_string()) == "source"
                });
            if let Some(s) = pair
                .child_by_field_name("value")
                .map(peel)
                .filter(|_| is_source)
                .and_then(|v| string_value(v, src))
            {
                out.push(s);
            }
        }
    }
}
