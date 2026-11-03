//! NestJS controllers and guards (Learn spec §6.3), and the calls through constructor
//! injection, which apply to any TypeScript class. Conventions, checked against the
//! official docs:
//! - <https://docs.nestjs.com/controllers> (`@Controller('prefix')`, the method decorators
//!   `@Get @Post @Put @Delete @Patch @Options @Head @All`, `:id` parameters, wildcards);
//! - <https://docs.nestjs.com/faq/global-prefix> (`app.setGlobalPrefix('v1')`; its
//!   `exclude` option is a documented limit);
//! - <https://docs.nestjs.com/guards> (`@UseGuards(G)` on a method or its controller);
//! - <https://docs.nestjs.com/providers> (constructor-based injection).
//!
//! A path written as a template with `${…}` cannot be known: the controller or method is
//! skipped with a note. The guard edges start from the routes `derive` kept, found again by
//! their file and line, so a route dropped as a duplicate lends no guard to the kept one.

use std::collections::{BTreeMap, BTreeSet};

use hm_core::ids::file_id;

use super::express::{is_literal, path_pattern};
use super::{Derived, Ids, InferredEdge, Inputs, Route, route};
use crate::facts::{ArgFact, FileFacts, Owner, SymbolKind};
use crate::ts::resolve::{Project, binding_package, resolve_callee};

/// The method decorators and their verb.
const VERBS: [(&str, &str); 8] = [
    ("Get", "GET"),
    ("Post", "POST"),
    ("Put", "PUT"),
    ("Patch", "PATCH"),
    ("Delete", "DELETE"),
    ("Options", "OPTIONS"),
    ("Head", "HEAD"),
    ("All", "ANY"),
];

/// The routes of `@Controller` classes.
pub(crate) fn routes(inputs: &Inputs, out: &mut Derived) {
    let mut ids = Ids::new(inputs.project);
    let (found, notes) = controller_routes(inputs, &mut ids);
    out.routes.extend(found);
    out.notes.extend(notes);
}

/// The `ts.nest.guard` edges from each route to its guards, and the `ts.di.constructor`
/// edges.
pub(crate) fn before_edges(inputs: &Inputs, out: &mut Derived) {
    let project = inputs.project;
    let mut ids = Ids::new(project);
    // `(route id, file, class, method)` of every kept controller route: its method
    // decorator is the one on the route's line with the route's verb.
    let mut handled: Vec<(&str, &str, &str, &str)> = Vec::new();
    for r in out.routes.iter().filter(|r| r.rule == "ts.nest.route") {
        let Some(facts) = project.facts.get(&r.file) else {
            continue;
        };
        let decorator = facts.web.decorators.iter().find(|d| {
            d.line == r.line
                && !d.target.0.is_empty()
                && verb_of(project, &r.file, &d.name) == Some(r.method.as_str())
        });
        if let Some(d) = decorator {
            handled.push((&r.id, &r.file, &d.target.0, &d.target.1));
        }
    }
    let mut edges = BTreeSet::new();
    for (rel, facts) in project.facts {
        for d in &facts.web.decorators {
            if nest_name(project, rel, &d.name).as_deref() != Some("UseGuards") {
                continue;
            }
            // A class decorator has `("", class)`, a method one `(class, method)`.
            let (class, method) = match &d.target {
                (empty, class) if empty.is_empty() => (class, None),
                (class, method) => (class, Some(method)),
            };
            for arg in &d.args {
                let ArgFact::Name(name) = arg else {
                    continue;
                };
                let Some(guard) = named_class(project, &mut ids, rel, name) else {
                    continue;
                };
                for (id, _, _, _) in handled.iter().filter(|(_, file, c, m)| {
                    file == rel && c == class && method.is_none_or(|method| method == m)
                }) {
                    edges.insert(InferredEdge {
                        kind: "calls".to_string(),
                        src_id: id.to_string(),
                        dst_id: guard.clone(),
                        file: rel.clone(),
                        line: d.line,
                        rule: "ts.nest.guard".to_string(),
                    });
                }
            }
        }
    }
    out.edges.extend(edges);
    injection_edges(project, &mut ids, out);
}

/// The name a decorator has in `@nestjs/common` when the file imports it from there
/// (`Get`, also for `import { Get as G }`).
fn nest_name(project: &Project, rel: &str, decorator: &str) -> Option<String> {
    match binding_package(project, rel, decorator)? {
        (package, imported) if package == "@nestjs/common" => Some(imported),
        _ => None,
    }
}

/// The verb of a method decorator from `@nestjs/common` (`ANY` for `All`).
fn verb_of(project: &Project, rel: &str, decorator: &str) -> Option<&'static str> {
    let name = nest_name(project, rel, decorator)?;
    VERBS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, verb)| *verb)
}

/// The id of the class that `name` (`Guard` or `ns.Guard`) names from `rel`.
fn named_class(project: &Project, ids: &mut Ids, rel: &str, name: &str) -> Option<String> {
    let (receiver, callee) = match name.rsplit_once('.') {
        Some((receiver, callee)) => (Some(receiver), callee),
        None => (None, name),
    };
    let (file, index) = resolve_callee(project, rel, receiver, callee)?;
    (project.facts[&file].symbols[index].kind == SymbolKind::Class)
        .then(|| ids.symbol(&file, index))?
}

/// The path a `@Controller` or method decorator gives: none, a string, or the `path` of
/// an options object; `None` when it cannot be read (an array, a variable, a template with
/// `${…}`).
fn decorator_path(args: &[ArgFact]) -> Option<String> {
    match args.first() {
        None => Some(String::new()),
        Some(ArgFact::Str(path)) => is_literal(path).then(|| path.clone()),
        Some(ArgFact::Object { path: Some(path) }) => is_literal(path).then(|| path.clone()),
        Some(ArgFact::Object { path: None }) => Some(String::new()),
        Some(_) => None,
    }
}

/// Every controller route of the tree, and the notes.
fn controller_routes(inputs: &Inputs, ids: &mut Ids) -> (Vec<Route>, Vec<String>) {
    let project = inputs.project;
    let (prefixes, mut notes) = global_prefixes(inputs);
    let mut found = Vec::new();
    for (rel, facts) in project.facts {
        let root = inputs
            .manifests
            .root_of(rel)
            .map_or("", |m| m.root.as_str());
        let prefix = prefixes.get(root).map_or("", String::as_str);
        let mut seen = BTreeSet::new();
        for c in &facts.web.decorators {
            if !c.target.0.is_empty()
                || nest_name(project, rel, &c.name).as_deref() != Some("Controller")
                || !seen.insert(&c.target.1)
            {
                continue;
            }
            let class = &c.target.1;
            let Some(class_path) = decorator_path(&c.args) else {
                notes.push(format!(
                    "{rel}:{}: the path of controller `{class}` is not a string; its routes \
                     are skipped",
                    c.line
                ));
                continue;
            };
            for d in facts.web.decorators.iter().filter(|d| d.target.0 == *class) {
                let Some(verb) = verb_of(project, rel, &d.name) else {
                    continue;
                };
                let method = &d.target.1;
                let Some(method_path) = decorator_path(&d.args) else {
                    notes.push(format!(
                        "{rel}:{}: the path of `{class}.{method}` is not a string; it is \
                         skipped",
                        d.line
                    ));
                    continue;
                };
                let pattern = path_pattern(&[prefix, &class_path, &method_path]);
                found.push(Route {
                    handler_id: method_symbol(facts, class, method)
                        .and_then(|i| ids.symbol(rel, i)),
                    ..route(root, "http", verb, &pattern, rel, d.line, "ts.nest.route")
                });
            }
        }
    }
    (found, notes)
}

/// The index of the one method `name` of `class` in `facts`.
fn method_symbol(facts: &FileFacts, class: &str, name: &str) -> Option<usize> {
    let mut found = facts
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind == SymbolKind::Method && s.owner == class && s.name == name)
        .map(|(i, _)| i);
    match (found.next(), found.next()) {
        (Some(i), None) => Some(i),
        _ => None,
    }
}

/// A `setGlobalPrefix` call: the prefix when it is a string, the file and the line.
type PrefixCall<'a> = (Option<&'a str>, &'a str, u32);

/// The global prefix of each manifest root: the string of a `setGlobalPrefix` call on a
/// binding from `NestFactory.create` (from `@nestjs/core`) in any file under that root.
/// Two different prefixes, or one that is not a literal, give none, with a note.
fn global_prefixes(inputs: &Inputs) -> (BTreeMap<String, String>, Vec<String>) {
    let project = inputs.project;
    // Root → every call in file and line order.
    let mut found: BTreeMap<&str, Vec<PrefixCall>> = BTreeMap::new();
    for (rel, facts) in project.facts {
        let from_core = matches!(
            binding_package(project, rel, "NestFactory"),
            Some((package, imported)) if package == "@nestjs/core" && imported == "NestFactory"
        );
        if !from_core {
            continue;
        }
        let apps: Vec<(Owner, &str)> = facts
            .web
            .bindings
            .iter()
            .filter(|b| b.init == "NestFactory.create")
            .map(|b| {
                let own = (String::new(), b.name.clone());
                (b.owner.clone().filter(|o| *o != own), b.name.as_str())
            })
            .collect();
        let root = inputs
            .manifests
            .root_of(rel)
            .map_or("", |m| m.root.as_str());
        for c in &facts.web.route_calls {
            let on_app = apps
                .iter()
                .any(|(owner, name)| *name == c.receiver && (owner.is_none() || *owner == c.owner));
            if c.method != "setGlobalPrefix" || !on_app {
                continue;
            }
            let prefix = match c.args.first() {
                Some(ArgFact::Str(p)) if !p.contains("{}") => Some(p.as_str()),
                _ => None,
            };
            found.entry(root).or_default().push((prefix, rel, c.line));
        }
    }

    let mut prefixes = BTreeMap::new();
    let mut notes = Vec::new();
    for (root, calls) in found {
        let (first, first_rel, first_line) = calls[0];
        let Some(first) = first else {
            notes.push(format!(
                "{first_rel}:{first_line}: the global prefix is not a string; no global \
                 prefix is applied"
            ));
            continue;
        };
        match calls.iter().find(|(p, _, _)| *p != Some(first)) {
            None => {
                prefixes.insert(root.to_string(), first.to_string());
            }
            Some((Some(other), rel, line)) => notes.push(format!(
                "{rel}:{line}: a second global prefix `{other}` (also `{first}` at \
                 {first_rel}:{first_line}); no global prefix is applied"
            )),
            Some((None, rel, line)) => notes.push(format!(
                "{rel}:{line}: the global prefix is not a string; no global prefix is applied"
            )),
        }
    }
    (prefixes, notes)
}

/// `this.<field>.<method>()` inside a class whose constructor declares `field: T`, with `T`
/// a class (of this file or imported) that has exactly one method of that name: a `calls`
/// edge from the calling symbol to that method. An interface type names no class.
fn injection_edges(project: &Project, ids: &mut Ids, out: &mut Derived) {
    let mut edges = BTreeSet::new();
    for (rel, facts) in project.facts {
        if facts.web.ctor_fields.is_empty() {
            continue;
        }
        for c in &facts.calls {
            let (Some(field), Some((class, _))) = (
                c.receiver.as_deref().and_then(|r| r.strip_prefix("this.")),
                c.owner.as_ref(),
            ) else {
                continue;
            };
            let mut types = facts
                .web
                .ctor_fields
                .iter()
                .filter(|f| f.class == *class && f.field == field);
            let (Some(ty), None) = (types.next(), types.next()) else {
                continue;
            };
            let Some((file, index)) = resolve_callee(project, rel, None, &ty.type_name) else {
                continue;
            };
            let target = &project.facts[&file];
            if target.symbols[index].kind != SymbolKind::Class {
                continue;
            }
            let Some(method) = method_symbol(target, &target.symbols[index].name, &c.callee) else {
                continue;
            };
            let Some(dst) = ids.symbol(&file, method) else {
                continue;
            };
            let src = ids
                .holder(rel, c.owner.as_ref(), c.line)
                .unwrap_or_else(|| file_id(rel));
            edges.insert(InferredEdge {
                kind: "calls".to_string(),
                src_id: src,
                dst_id: dst,
                file: rel.clone(),
                line: c.line,
                rule: "ts.di.constructor".to_string(),
            });
        }
    }
    out.edges.extend(edges);
}
